//! The JIT: hot regions of decoded instructions compiled to native code
//! with Cranelift (not in the WebAssembly build).
//!
//! A region is a set of basic blocks in one page, entered at its first
//! block. Guest registers and the flags are Cranelift variables, so they
//! live in host registers inside the region; they are loaded at entry and
//! written back at every exit. ADD/SUB/CMP, LI, LEA, branches, BR3 and JAL
//! are compiled inline (jumps within the region are native jumps), and so
//! are word loads and stores through the CPU's read and write windows,
//! with a helper call when the window misses. Any other non-control
//! instruction calls the interpreter's own code for it, with its registers
//! synced around the call. JALR, generic instructions, faults, a store that
//! touches code or a page table, and too little budget for the next block
//! all exit to the block engine, which carries on from the exact
//! instruction.
//!
//! The S flag is kept lazily: a variable holds a value whose sign is S (the
//! last result, or the difference of a comparison), and the sign is only
//! taken where S is read. C is kept as the trit itself.
//!
//! A compiled region is `fn(cpu, mem, va_base, budget) -> steps`:
//! `va_base` is the virtual address of the region's first instruction (the
//! same code may be mapped at several addresses), `budget` the steps it may
//! take. It returns the steps taken, with `cpu.pc` set; a fault leaves
//! `cpu.jit_fault` set and the PC on the faulting instruction (not counted).

use super::*;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::{I64, I8};
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};
use std::collections::HashMap;

pub(super) type NativeFn = unsafe extern "C" fn(*mut Cpu, *mut Memory, i64, i64) -> i64;

/// One basic block of a region: its offset in trytes from the region's
/// start, and its instructions.
pub(super) struct RegionBlock {
    pub off: i64,
    pub ops: Vec<Dec>,
}

pub(super) struct Jit {
    module: JITModule,
    ctx: cranelift_codegen::Context,
    fctx: FunctionBuilderContext,
    load_word: FuncId,
    store_word: FuncId,
    exec_one: FuncId,
    next_id: usize,
}

// ---- helpers called from native code ----------------------------------------

/// Status codes from helpers.
const OK: i64 = 0;
const OK_EXIT: i64 = 1; // done, but it wrote code or a page table: leave
const FAULT: i64 = 2;

unsafe extern "C" fn jit_load_word(cpu: *mut Cpu, mem: *mut Memory, va: i64, out: *mut i64) -> i64 {
    let (cpu, m) = (&mut *cpu, &mut *mem);
    match cpu.load_word_fast(m, va) {
        Ok(v) => {
            *out = v.value();
            OK
        }
        Err(f) => {
            cpu.jit_fault = Some(f);
            FAULT
        }
    }
}

unsafe extern "C" fn jit_store_word(cpu: *mut Cpu, mem: *mut Memory, va: i64, v: i64) -> i64 {
    let (cpu, m) = (&mut *cpu, &mut *mem);
    match cpu.store_word_fast(m, va, Word::new(v).expect("a word")) {
        Ok(()) if !m.code_dirty.is_empty() || m.pt_dirty => OK_EXIT,
        Ok(()) => OK,
        Err(f) => {
            cpu.jit_fault = Some(f);
            FAULT
        }
    }
}

/// Any non-control instruction, by the interpreter (its registers and the
/// flags are in `cpu` around the call).
unsafe extern "C" fn jit_exec_one(cpu: *mut Cpu, mem: *mut Memory, d: *const Dec, pc: i64) -> i64 {
    let (cpu, m) = (&mut *cpu, &mut *mem);
    match cpu.exec_op(m, &*d, pc) {
        Ok(_) if !m.code_dirty.is_empty() || m.pt_dirty => OK_EXIT,
        Ok(_) => OK,
        Err(f) => {
            cpu.jit_fault = Some(f);
            FAULT
        }
    }
}

// ---- code generation --------------------------------------------------------------

type IrBlock = cranelift_codegen::ir::Block;

/// What one region's code generation needs at hand.
struct Gen<'a> {
    b: FunctionBuilder<'a>,
    cpu: Value,
    mem: Value,
    va_base: Value,
    reg: [Option<Variable>; 27],
    s_var: Variable,
    c_var: Variable,
    left: Variable,
    exit: IrBlock,
    exit_pc: Variable,
    exit_unrun: Variable,
    flags_live: bool,
}

const FL: MemFlagsData = MemFlagsData::trusted();

impl Gen<'_> {
    fn get(&mut self, r: i64) -> Value {
        if r == 0 {
            self.b.ins().iconst(I64, 0)
        } else {
            self.b.use_var(self.reg[idx(r)].expect("used register"))
        }
    }

    fn set(&mut self, r: i64, v: Value) {
        if r != 0 {
            self.b.def_var(self.reg[idx(r)].expect("used register"), v);
        }
    }

    /// −1, 0 or +1.
    fn sign(&mut self, v: Value) -> Value {
        let pos = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, v, 0);
        let neg = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, v, 0);
        let p = self.b.ins().uextend(I64, pos);
        let n = self.b.ins().uextend(I64, neg);
        self.b.ins().isub(p, n)
    }

    /// Wrap a sum of a few words: (word, carry trit). The word comes from
    /// two selects, so dependent adds wait only on compares and moves.
    fn wrap(&mut self, v: Value) -> (Value, Value) {
        let over = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, v, W_MAX);
        let under = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, v, -W_MAX);
        let down = self.b.ins().iadd_imm_s(v, -W_MOD);
        let up = self.b.ins().iadd_imm_s(v, W_MOD);
        let v1 = self.b.ins().select(over, down, v);
        let w = self.b.ins().select(under, up, v1);
        let o = self.b.ins().uextend(I64, over);
        let u = self.b.ins().uextend(I64, under);
        (w, self.b.ins().isub(o, u))
    }

    /// A word product split in two: (high word, low word), as the
    /// interpreter's `split_product27` (an f64 quotient estimate, within 1,
    /// then one exact correction each way).
    fn mul_split(&mut self, x: Value, y: Value) -> (Value, Value) {
        use cranelift_codegen::ir::types::F64;
        let fx = self.b.ins().fcvt_from_sint(F64, x);
        let fy = self.b.ins().fcvt_from_sint(F64, y);
        let p = self.b.ins().fmul(fx, fy);
        let m = self.b.ins().f64const(W_MOD as f64);
        let q = self.b.ins().fdiv(p, m);
        let q = self.b.ins().nearest(q);
        let hi = self.b.ins().fcvt_to_sint_sat(I64, q);
        let xy = self.b.ins().imul(x, y);
        let hm = self.b.ins().imul_imm_s(hi, W_MOD);
        let lo = self.b.ins().isub(xy, hm);
        let over = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, lo, W_MAX);
        let under = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, lo, -W_MAX);
        let lo_d = self.b.ins().iadd_imm_s(lo, -W_MOD);
        let lo_u = self.b.ins().iadd_imm_s(lo, W_MOD);
        let lo1 = self.b.ins().select(over, lo_d, lo);
        let lo2 = self.b.ins().select(under, lo_u, lo1);
        let o = self.b.ins().uextend(I64, over);
        let u = self.b.ins().uextend(I64, under);
        let hi1 = self.b.ins().iadd(hi, o);
        let hi2 = self.b.ins().isub(hi1, u);
        (hi2, lo2)
    }

    /// The address of the instruction at `off` (within the page: no wrap).
    fn pc_in_page(&mut self, off: i64) -> Value {
        self.b.ins().iadd_imm_s(self.va_base, off)
    }

    /// The address `off` from the start, which may lie outside the page.
    fn pc_any(&mut self, off: i64) -> Value {
        let v = self.b.ins().iadd_imm_s(self.va_base, off);
        self.wrap(v).0
    }

    fn leave(&mut self, pc: Value, unrun: i64) {
        let u = self.b.ins().iconst(I64, unrun);
        self.b.def_var(self.exit_pc, pc);
        self.b.def_var(self.exit_unrun, u);
        self.b.ins().jump(self.exit, &[]);
    }

    /// Go to the region's block at `off`, or leave for it.
    fn goto(&mut self, blocks: &HashMap<i64, IrBlock>, off: i64) {
        match blocks.get(&off) {
            Some(&tb) => {
                self.b.ins().jump(tb, &[]);
            }
            None => {
                let pc = self.pc_any(off);
                self.leave(pc, 0);
            }
        }
    }

    fn flags_to_cpu(&mut self) {
        if self.flags_live {
            let sv = self.b.use_var(self.s_var);
            let s = self.sign(sv);
            let c = self.b.use_var(self.c_var);
            self.b.ins().istore8(FL, s, self.cpu, (OFF_MODE + mode::S) as i32);
            self.b.ins().istore8(FL, c, self.cpu, (OFF_MODE + mode::C) as i32);
        }
    }

    fn flags_from_cpu(&mut self) {
        if self.flags_live {
            let s = self.b.ins().sload8(I64, FL, self.cpu, (OFF_MODE + mode::S) as i32);
            let c = self.b.ins().sload8(I64, FL, self.cpu, (OFF_MODE + mode::C) as i32);
            self.b.def_var(self.s_var, s);
            self.b.def_var(self.c_var, c);
        }
    }

    /// The window test for a word access at `va` through the window at
    /// `lo_off`/`ptr_off`: (covered?, the word's host address).
    fn window(&mut self, va: Value, lo_off: usize, ptr_off: usize) -> (Value, Value) {
        let lo = self.b.ins().load(I64, FL, self.cpu, lo_off as i32);
        let ptr = self.b.ins().load(I64, FL, self.cpu, ptr_off as i32);
        let off = self.b.ins().isub(va, lo);
        // q = off / 3 for 0 ≤ off < 2^16, r = off − 3q (a word centre: 1)
        let q = self.b.ins().imul_imm_s(off, 43_691);
        let q = self.b.ins().ushr_imm_s(q, 17);
        let q3 = self.b.ins().imul_imm_s(q, 3);
        let r = self.b.ins().isub(off, q3);
        let in_page = self.b.ins().icmp_imm_u(IntCC::UnsignedLessThan, off, PAGE);
        let centre = self.b.ins().icmp_imm_s(IntCC::Equal, r, 1);
        let has = self.b.ins().icmp_imm_s(IntCC::NotEqual, ptr, 0);
        let dirty = self.b.ins().load(I8, FL, self.mem, OFF_PT_DIRTY as i32);
        let clean = self.b.ins().icmp_imm_s(IntCC::Equal, dirty, 0);
        let a = self.b.ins().band(in_page, centre);
        let c = self.b.ins().band(has, clean);
        let ok = self.b.ins().band(a, c);
        let byte = self.b.ins().ishl_imm_s(q, 3);
        let addr = self.b.ins().iadd(ptr, byte);
        (ok, addr)
    }
}

impl Jit {
    pub(super) fn new() -> Option<Jit> {
        let mut flags = settings::builder();
        flags.set("opt_level", "speed").ok()?;
        flags.set("use_colocated_libcalls", "false").ok()?;
        flags.set("is_pic", "false").ok()?;
        let isa = cranelift_native::builder().ok()?.finish(settings::Flags::new(flags)).ok()?;
        let mut jb = JITBuilder::with_isa(isa, default_libcall_names());
        jb.symbol("jit_load_word", jit_load_word as *const u8);
        jb.symbol("jit_store_word", jit_store_word as *const u8);
        jb.symbol("jit_exec_one", jit_exec_one as *const u8);
        let mut module = JITModule::new(jb);
        let ptr = module.target_config().pointer_type();
        let mut decl = |name: &str, params: [cranelift_codegen::ir::Type; 4]| -> Option<FuncId> {
            let mut sig = module.make_signature();
            for t in params {
                sig.params.push(AbiParam::new(t));
            }
            sig.returns.push(AbiParam::new(I64));
            module.declare_function(name, Linkage::Import, &sig).ok()
        };
        let load_word = decl("jit_load_word", [ptr, ptr, I64, ptr])?;
        let store_word = decl("jit_store_word", [ptr, ptr, I64, I64])?;
        let exec_one = decl("jit_exec_one", [ptr, ptr, ptr, I64])?;
        Some(Jit {
            ctx: module.make_context(),
            module,
            fctx: FunctionBuilderContext::new(),
            load_word,
            store_word,
            exec_one,
            next_id: 0,
        })
    }

    /// Compile a region. `ops_keep` receives the decoded instructions the
    /// native code points at (they must live as long as it does, at a fixed
    /// address: hence the boxes).
    #[allow(clippy::vec_box)]
    pub(super) fn compile(&mut self, region: &[RegionBlock], ops_keep: &mut Vec<Box<Dec>>) -> Option<NativeFn> {
        let tcfg = self.module.target_config();
        let ptr = tcfg.pointer_type();
        self.module.clear_context(&mut self.ctx);
        for t in [ptr, ptr, I64, I64] {
            self.ctx.func.signature.params.push(AbiParam::new(t));
        }
        self.ctx.func.signature.returns.push(AbiParam::new(I64));
        let name = format!("region{}", self.next_id);
        self.next_id += 1;
        let fid = self.module.declare_function(&name, Linkage::Local, &self.ctx.func.signature).ok()?;
        self.ctx.func.name = UserFuncName::user(0, fid.as_u32());

        let (load_word, store_word, exec_one) = (self.load_word, self.store_word, self.exec_one);
        let mut b = FunctionBuilder::new(&mut self.ctx.func, &mut self.fctx);
        let lw = self.module.declare_func_in_func(load_word, b.func);
        let sw = self.module.declare_func_in_func(store_word, b.func);
        let ex = self.module.declare_func_in_func(exec_one, b.func);

        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let p = b.block_params(entry).to_vec();
        let (cpu, mem, va_base, budget) = (p[0], p[1], p[2], p[3]);
        let slot = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));

        // which registers the region uses and writes
        let mut used = [false; 27];
        let mut written = [false; 27];
        let mut flags_live = false;
        for rb in region {
            for d in &rb.ops {
                for r in [d.x, d.y, d.z] {
                    used[idx(r as i64)] = true;
                }
                if !matches!(d.op, Op::Stt | Op::Stw | Op::Br | Op::Br3 | Op::CmpR | Op::CmpK | Op::Generic) {
                    written[idx(d.x as i64)] = true;
                }
                flags_live |= !matches!(d.op, Op::Li | Op::Lea | Op::Jal | Op::Jalr | Op::Ldw | Op::Stw | Op::Generic);
            }
        }
        used[idx(0)] = false;
        written[idx(0)] = false;

        let mut reg = [None::<Variable>; 27];
        for r in 0..27 {
            if used[r] {
                let v = b.declare_var(I64);
                let val = b.ins().load(I64, FL, cpu, (OFF_REGS + 8 * r) as i32);
                b.def_var(v, val);
                reg[r] = Some(v);
            }
        }
        let s_var = b.declare_var(I64);
        let c_var = b.declare_var(I64);
        let left = b.declare_var(I64);
        b.def_var(left, budget);
        let exit = b.create_block();
        let exit_pc = b.declare_var(I64);
        let exit_unrun = b.declare_var(I64);
        let mut g = Gen { b, cpu, mem, va_base, reg, s_var, c_var, left, exit, exit_pc, exit_unrun, flags_live };
        g.flags_from_cpu();

        let mut blocks = HashMap::new();
        for rb in region {
            blocks.insert(rb.off, g.b.create_block());
        }
        let first = blocks[&region[0].off];
        g.b.ins().jump(first, &[]);

        for rb in region {
            g.b.switch_to_block(blocks[&rb.off]);
            // a trailing generic instruction is not run here
            let count = rb.ops.iter().take_while(|d| d.op != Op::Generic).count() as i64;
            // leave before the block if the budget cannot cover it
            let l = g.b.use_var(g.left);
            let short = g.b.ins().icmp_imm_s(IntCC::SignedLessThan, l, count.max(1));
            let go = g.b.create_block();
            let stop = g.b.create_block();
            g.b.set_cold_block(stop);
            g.b.ins().brif(short, stop, &[], go, &[]);
            g.b.switch_to_block(stop);
            let pc = g.pc_in_page(rb.off);
            g.leave(pc, 0);
            g.b.switch_to_block(go);
            let l2 = g.b.ins().iadd_imm_s(l, -count);
            g.b.def_var(g.left, l2);

            let mut off = rb.off;
            let mut ended = false;
            for (k, d) in rb.ops.iter().enumerate() {
                let unrun = count - k as i64; // if we leave before op k
                let next_off = off + d.len as i64;
                let (x, y, z) = (d.x as i64, d.y as i64, d.z as i64);
                match d.op {
                    Op::AddR | Op::SubR | Op::AddK | Op::SubK => {
                        let a = g.get(y);
                        let c = match d.op {
                            Op::AddR | Op::SubR => g.get(z),
                            _ => g.b.ins().iconst(I64, d.imm),
                        };
                        let raw = if matches!(d.op, Op::AddR | Op::AddK) { g.b.ins().iadd(a, c) } else { g.b.ins().isub(a, c) };
                        let (v, carry) = g.wrap(raw);
                        g.b.def_var(g.c_var, carry);
                        g.b.def_var(g.s_var, v);
                        g.set(x, v);
                    }
                    Op::CmpR | Op::CmpK => {
                        let a = g.get(y);
                        let c = if d.op == Op::CmpR { g.get(z) } else { g.b.ins().iconst(I64, d.imm) };
                        let diff = g.b.ins().isub(a, c);
                        g.b.def_var(g.s_var, diff);
                    }
                    Op::AluR | Op::AluK if matches!(d.f as i64, alu::MUL | alu::MULH) => {
                        let a = g.get(y);
                        let c = if d.op == Op::AluR { g.get(z) } else { g.b.ins().iconst(I64, d.imm) };
                        let (hi, lo) = g.mul_split(a, c);
                        if d.f as i64 == alu::MUL {
                            let carry = g.sign(hi);
                            g.b.def_var(g.c_var, carry);
                            g.b.def_var(g.s_var, lo);
                            g.set(x, lo);
                        } else {
                            g.b.def_var(g.s_var, hi);
                            g.set(x, hi);
                        }
                    }
                    Op::AluR | Op::AluK if matches!(d.f as i64, alu::DIV | alu::MOD) && (d.op == Op::AluR || d.imm != 0) => {
                        // a zero divisor goes to the interpreter, which faults
                        let a = g.get(y);
                        let c = if d.op == Op::AluR { g.get(z) } else { g.b.ins().iconst(I64, d.imm) };
                        let zero = g.b.ins().icmp_imm_s(IntCC::Equal, c, 0);
                        let divide = g.b.create_block();
                        let fault = g.b.create_block();
                        g.b.set_cold_block(fault);
                        g.b.ins().brif(zero, fault, &[], divide, &[]);
                        g.b.switch_to_block(fault);
                        let pc = g.pc_in_page(off);
                        let keep = Box::new(*d);
                        let dp = g.b.ins().iconst(ptr, &*keep as *const Dec as i64);
                        ops_keep.push(keep);
                        g.b.ins().call(ex, &[cpu, mem, dp, pc]);
                        g.leave(pc, unrun);
                        g.b.switch_to_block(divide);
                        let r = if d.f as i64 == alu::DIV { g.b.ins().sdiv(a, c) } else { g.b.ins().srem(a, c) };
                        g.b.def_var(g.s_var, r);
                        g.set(x, r);
                    }
                    Op::Li => {
                        let v = g.b.ins().iconst(I64, d.imm);
                        g.set(x, v);
                    }
                    Op::Lea => {
                        let v = g.pc_any(off + d.imm);
                        g.set(x, v);
                    }
                    Op::Ldw | Op::Stw => {
                        let base = g.get(y);
                        let raw = g.b.ins().iadd_imm_s(base, d.imm);
                        let va = g.wrap(raw).0;
                        let fast = g.b.create_block();
                        let slow = g.b.create_block();
                        let done = g.b.create_block();
                        let (lo, pt) = if d.op == Op::Ldw { (OFF_RW_LO, OFF_RW_PTR) } else { (OFF_WW_LO, OFF_WW_PTR) };
                        let (ok, addr) = g.window(va, lo, pt);
                        g.b.set_cold_block(slow);
                        g.b.ins().brif(ok, fast, &[], slow, &[]);
                        // through the window
                        g.b.switch_to_block(fast);
                        if d.op == Op::Ldw {
                            let v = g.b.ins().load(I64, FL, addr, 0);
                            g.set(x, v);
                        } else {
                            let v = g.get(x);
                            g.b.ins().store(FL, v, addr, 0);
                        }
                        g.b.ins().jump(done, &[]);
                        // the helper: TLB, a new window, or a fault
                        g.b.switch_to_block(slow);
                        let status = if d.op == Op::Ldw {
                            let out = g.b.ins().stack_addr(ptr, slot, 0);
                            let call = g.b.ins().call(lw, &[cpu, mem, va, out]);
                            g.b.inst_results(call)[0]
                        } else {
                            let v = g.get(x);
                            let call = g.b.ins().call(sw, &[cpu, mem, va, v]);
                            g.b.inst_results(call)[0]
                        };
                        let fault = g.b.create_block();
                        let ok = g.b.create_block();
                        g.b.set_cold_block(fault);
                        let bad = g.b.ins().icmp_imm_s(IntCC::Equal, status, FAULT);
                        g.b.ins().brif(bad, fault, &[], ok, &[]);
                        g.b.switch_to_block(fault);
                        let pc = g.pc_in_page(off);
                        g.leave(pc, unrun);
                        g.b.switch_to_block(ok);
                        if d.op == Op::Ldw {
                            let v = g.b.ins().stack_load(ptr, I64, slot, 0);
                            g.set(x, v);
                            g.b.ins().jump(done, &[]);
                        } else {
                            // it wrote code or a page table: leave after it
                            let out = g.b.create_block();
                            let dirty = g.b.ins().icmp_imm_s(IntCC::Equal, status, OK_EXIT);
                            g.b.ins().brif(dirty, out, &[], done, &[]);
                            g.b.switch_to_block(out);
                            let pc = g.pc_in_page(next_off);
                            g.leave(pc, unrun - 1);
                        }
                        g.b.switch_to_block(done);
                    }
                    Op::Jal => {
                        let link = g.pc_any(next_off);
                        g.set(x, link);
                        g.goto(&blocks, off + 3 * d.imm);
                        ended = true;
                    }
                    Op::Br | Op::Br3 => {
                        let v = g.b.use_var(g.s_var);
                        let tests: Vec<(Value, i64)> = if d.op == Op::Br {
                            // taken when the flag's value has its mask bit set
                            let mut hits = Vec::new();
                            for k in 0..3i64 {
                                if d.imm2 >> k & 1 != 0 {
                                    let hit = if d.f == 1 {
                                        let cc = [IntCC::SignedLessThan, IntCC::Equal, IntCC::SignedGreaterThan][k as usize];
                                        g.b.ins().icmp_imm_s(cc, v, 0)
                                    } else {
                                        let c = g.b.use_var(g.c_var);
                                        g.b.ins().icmp_imm_s(IntCC::Equal, c, k - 1)
                                    };
                                    hits.push(hit);
                                }
                            }
                            let taken = match hits.split_first() {
                                None => g.b.ins().iconst(I8, 0),
                                Some((&h, rest)) => rest.iter().fold(h, |a, &h| g.b.ins().bor(a, h)),
                            };
                            vec![(taken, off + 3 * d.imm)]
                        } else {
                            let neg = g.b.ins().icmp_imm_s(IntCC::SignedLessThan, v, 0);
                            let pos = g.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, v, 0);
                            vec![(neg, off + 3 * d.imm), (pos, off + 3 * d.imm2 as i64)]
                        };
                        for (cond, t) in tests {
                            let yes = g.b.create_block();
                            let no = g.b.create_block();
                            g.b.ins().brif(cond, yes, &[], no, &[]);
                            g.b.switch_to_block(yes);
                            g.goto(&blocks, t);
                            g.b.switch_to_block(no);
                        }
                        g.goto(&blocks, next_off);
                        ended = true;
                    }
                    Op::Jalr => {
                        let base = g.get(y);
                        let raw = g.b.ins().iadd_imm_s(base, d.imm);
                        let target = g.wrap(raw).0;
                        let link = g.pc_any(next_off);
                        g.set(x, link);
                        g.leave(target, 0);
                        ended = true;
                    }
                    Op::Generic => {
                        let pc = g.pc_in_page(off);
                        g.leave(pc, 0);
                        ended = true;
                    }
                    _ => {
                        // the interpreter's code for this one instruction
                        for r in [x, y, z] {
                            if r != 0 {
                                let v = g.get(r);
                                g.b.ins().store(FL, v, cpu, (OFF_REGS + 8 * idx(r)) as i32);
                            }
                        }
                        g.flags_to_cpu();
                        let keep = Box::new(*d);
                        let dp = g.b.ins().iconst(ptr, &*keep as *const Dec as i64);
                        ops_keep.push(keep);
                        let pc = g.pc_in_page(off);
                        let call = g.b.ins().call(ex, &[cpu, mem, dp, pc]);
                        let status = g.b.inst_results(call)[0];
                        let fault = g.b.create_block();
                        let ok = g.b.create_block();
                        g.b.set_cold_block(fault);
                        let bad = g.b.ins().icmp_imm_s(IntCC::Equal, status, FAULT);
                        g.b.ins().brif(bad, fault, &[], ok, &[]);
                        g.b.switch_to_block(fault);
                        g.leave(pc, unrun);
                        g.b.switch_to_block(ok);
                        if x != 0 {
                            let v = g.b.ins().load(I64, FL, cpu, (OFF_REGS + 8 * idx(x)) as i32);
                            g.set(x, v);
                        }
                        g.flags_from_cpu();
                        let out = g.b.create_block();
                        let on = g.b.create_block();
                        let dirty = g.b.ins().icmp_imm_s(IntCC::Equal, status, OK_EXIT);
                        g.b.ins().brif(dirty, out, &[], on, &[]);
                        g.b.switch_to_block(out);
                        let pc = g.pc_in_page(next_off);
                        g.leave(pc, unrun - 1);
                        g.b.switch_to_block(on);
                    }
                }
                if ended {
                    break;
                }
                off = next_off;
            }
            if !ended {
                g.goto(&blocks, off); // fell off the end (length limit)
            }
        }

        // exit: registers, flags, PC; steps = budget − left − unrun
        g.b.switch_to_block(exit);
        for (r, &w) in written.iter().enumerate() {
            if w {
                let v = g.b.use_var(g.reg[r].expect("used register"));
                g.b.ins().store(FL, v, cpu, (OFF_REGS + 8 * r) as i32);
            }
        }
        g.flags_to_cpu();
        let pc = g.b.use_var(g.exit_pc);
        g.b.ins().store(FL, pc, cpu, OFF_PC as i32);
        let l = g.b.use_var(g.left);
        let u = g.b.use_var(g.exit_unrun);
        let ran = g.b.ins().isub(budget, l);
        let ran = g.b.ins().isub(ran, u);
        g.b.ins().return_(&[ran]);
        g.b.seal_all_blocks();
        g.b.finalize(tcfg);

        self.module.define_function(fid, &mut self.ctx).ok()?;
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions().ok()?;
        let code = self.module.get_finalized_function(fid);
        Some(unsafe { std::mem::transmute::<*const u8, NativeFn>(code) })
    }
}

// ---- the compiler thread ------------------------------------------------------------

/// A region to compile, and the result.
pub(super) struct Request {
    pub id: u64,
    pub region: Vec<RegionBlock>,
}

pub(super) struct Done {
    pub id: u64,
    /// The compiled code (a `NativeFn`), or 0 if it failed.
    pub code: usize,
    pub micros: u64,
}

/// Cranelift on its own thread: the CPU keeps running while regions
/// compile, and installs the code when it arrives.
pub(super) struct JitThread {
    /// One request channel per compiler thread, used in turn.
    tx: Vec<std::sync::mpsc::Sender<Request>>,
    next: std::cell::Cell<usize>,
    rx: std::sync::mpsc::Receiver<Done>,
}

/// Compiler threads: regions compile in parallel, so hot code waits less.
const JIT_THREADS: usize = 4;

impl JitThread {
    pub(super) fn spawn() -> Option<JitThread> {
        let (done_tx, rx) = std::sync::mpsc::channel::<Done>();
        let mut tx = Vec::new();
        for k in 0..JIT_THREADS {
            let (req_tx, req_rx) = std::sync::mpsc::channel::<Request>();
            let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();
            let done_tx = done_tx.clone();
            std::thread::Builder::new()
                .name(format!("setun-jit-{k}"))
                .spawn(move || {
                    let Some(mut jit) = Jit::new() else {
                        let _ = ready_tx.send(false);
                        return;
                    };
                    let _ = ready_tx.send(true);
                    // decoded instructions the code points at live here, with it
                    let mut keep = Vec::new();
                    for req in req_rx {
                        let t0 = std::time::Instant::now();
                        let code = jit.compile(&req.region, &mut keep).map_or(0, |f| f as usize);
                        let done = Done { id: req.id, code, micros: t0.elapsed().as_micros() as u64 };
                        if done_tx.send(done).is_err() {
                            break;
                        }
                    }
                })
                .ok()?;
            if !ready_rx.recv().ok()? {
                return None;
            }
            tx.push(req_tx);
        }
        Some(JitThread { tx, next: std::cell::Cell::new(0), rx })
    }

    pub(super) fn send(&self, r: Request) -> bool {
        let k = self.next.get();
        self.next.set((k + 1) % self.tx.len());
        self.tx[k].send(r).is_ok()
    }

    pub(super) fn try_recv(&self) -> Option<Done> {
        self.rx.try_recv().ok()
    }

    pub(super) fn recv(&self) -> Option<Done> {
        self.rx.recv().ok()
    }
}
