//! The JIT: hot regions of decoded instructions compiled to native code
//! with Cranelift (not in the WebAssembly build).
//!
//! A region is a set of basic blocks in one page, entered at any of them
//! (a batch can end anywhere in a loop): the run loop passes the entry
//! block's offset from the region's root, and every block of the region
//! points at its code. Guest registers and the flags are Cranelift variables, so they
//! live in host registers inside the region; they are loaded at entry,
//! and every register write is also stored to the CPU, so exits only
//! store the flags and the PC. ADD/SUB/CMP, LI, LEA, branches, BR3 and JAL
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
//! taken where S is read. C likewise: a raw (unwrapped) sum whose carry is
//! C.
//!
//! A compiled region is `fn(cpu, mem, va_base, budget) -> steps`:
//! `va_base` is the virtual address of the region's first instruction (the
//! same code may be mapped at several addresses), `budget` the steps it may
//! take. It returns the steps taken, with `cpu.pc` set; a fault leaves
//! `cpu.jit_fault` set and the PC on the faulting instruction (not counted).

use super::*;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::{I128, I64, I8};
use cranelift_codegen::ir::{AbiParam, FuncRef, InstBuilder, MemFlagsData, StackSlot, StackSlotData, StackSlotKind, UserFuncName, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};
use std::collections::HashMap;

/// (cpu, memory, the region root's address, budget, the entry block's
/// offset from the root) -> steps run.
pub(super) type NativeFn = unsafe extern "C" fn(*mut Cpu, *mut Memory, i64, i64, i64) -> i64;

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
    load_tryte: FuncId,
    store_tryte: FuncId,
    exec_one: FuncId,
    alu: FuncId,
    fcmp: FuncId,
    real_words: FuncId,
    fill_window: FuncId,
    next_id: usize,
}

// ---- helpers called from native code ----------------------------------------

/// Status codes from helpers.
const OK: i64 = 0;
const OK_EXIT: i64 = 1; // done, but it wrote code or a page table: leave
const FAULT: i64 = 2;

unsafe extern "C" fn jit_load_word(cpu: *mut Cpu, mem: *mut Memory, va: i64, out: *mut i64) -> i64 {
    let (cpu, m) = (&mut *cpu, &mut *mem);
    // compiled code passes base + offset unwrapped (the page walk would
    // not mind, but the windows must stay in range)
    let raw = va;
    let va = wrap_carry(va).0;
    match cpu.load_word_fast(m, va) {
        Ok(v) => {
            *out = v.value();
            cpu.jw_fill(raw, false);
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
    let raw = va;
    let va = wrap_carry(va).0;
    match cpu.store_word_fast(m, va, Word::new(v).expect("a word")) {
        Ok(()) if !m.code_dirty.is_empty() || m.pt_dirty => OK_EXIT,
        Ok(()) => {
            cpu.jw_fill(raw, true);
            OK
        }
        Err(f) => {
            cpu.jit_fault = Some(f);
            FAULT
        }
    }
}

/// A tryte load, likewise (LDT: the tryte, sign-extended).
unsafe extern "C" fn jit_load_tryte(cpu: *mut Cpu, mem: *mut Memory, va: i64, out: *mut i64) -> i64 {
    let (cpu, m) = (&mut *cpu, &mut *mem);
    let raw = va;
    let va = wrap_carry(va).0;
    let r = cpu
        .data_pa(m, va, Access::Read, false)
        .and_then(|pa| m.read_tryte(pa).map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Read)));
    match r {
        Ok(t) => {
            *out = t.value();
            cpu.jw_fill(raw, false);
            OK
        }
        Err(f) => {
            cpu.jit_fault = Some(f);
            FAULT
        }
    }
}

/// A tryte store (STT: the low tryte of `v`).
unsafe extern "C" fn jit_store_tryte(cpu: *mut Cpu, mem: *mut Memory, va: i64, v: i64) -> i64 {
    let (cpu, m) = (&mut *cpu, &mut *mem);
    let raw = va;
    let va = wrap_carry(va).0;
    let r = cpu.data_pa(m, va, Access::Write, false).and_then(|pa| {
        m.write_tryte(pa, Word::new(v).expect("a word").resize())
            .map_err(|_| Fault::mem(vector::BUS_ERROR, va, Access::Write))
    });
    match r {
        Ok(()) if !m.code_dirty.is_empty() || m.pt_dirty => OK_EXIT,
        Ok(()) => {
            cpu.jw_fill(raw, true);
            OK
        }
        Err(f) => {
            cpu.jit_fault = Some(f);
            FAULT
        }
    }
}

/// The ALU functions compiled code calls directly: pure, setting no flag
/// but S (which is the result's sign), never faulting.
fn alu_by_helper(f: i64) -> bool {
    matches!(f, alu::MIN | alu::MAX | alu::TMUL | alu::TSAT | alu::CONS | alu::SH | alu::TRIT | alu::TLZ | alu::BSH)
}

/// One of those (`x`, `y`: words).
extern "C" fn jit_alu(f: i64, x: i64, y: i64) -> i64 {
    let (a, b) = (Word::new(x).expect("a word"), Word::new(y).expect("a word"));
    let r = match f {
        alu::MIN => a.tritwise_min(b),
        alu::MAX => a.tritwise_max(b),
        alu::TMUL => a.tritwise_mul(b),
        alu::TSAT => a.tritwise_sat(b),
        alu::CONS => a.tritwise_cons(b),
        alu::SH => a.shift(y.clamp(-40, 40) as i32),
        alu::TRIT if (1..=27).contains(&y) => Word::new(a.trit(y as usize) as i64).expect("a trit"),
        alu::TRIT => Word::ZERO,
        alu::BSH => Word::new(crate::isa::bin_alu(f, x, y)).expect("wrapped"),
        _ => Word::new(a.leading_zero_trits() as i64).expect("a count"), // TLZ
    };
    r.value()
}

/// FCMP fy, fz: −1, 0 or +1, or 2 if unordered (a NaN).
unsafe extern "C" fn jit_fcmp(cpu: *const Cpu, y: i64, z: i64, _: i64) -> i64 {
    let cpu = &*cpu;
    match cpu.freg(y).cmp(cpu.freg(z)) {
        Some(o) => o as i64,
        None => 2,
    }
}

/// The three words of fx as a real81 (FSTD stores them).
unsafe extern "C" fn jit_real_words(cpu: *const Cpu, x: i64, out: *mut i64, _: i64) -> i64 {
    let ws = (*cpu).freg(x).to_words();
    std::ptr::copy_nonoverlapping(ws.as_ptr(), out, 3);
    OK
}

/// After the interpreter's access at `va` went through: its page into the
/// window table (`write`: the write half).
unsafe extern "C" fn jit_fill_window(cpu: *mut Cpu, va: i64, write: i64, _: i64) -> i64 {
    (*cpu).jw_fill(va, write != 0);
    OK
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

/// From fpu_inline: the slow block, the block both paths go on to, and the
/// address of a load or store (true: a store) for the slow path to open.
type FpuInline = (IrBlock, IrBlock, Option<(Value, bool)>);

/// What one region's code generation needs at hand.
struct Gen<'a> {
    b: FunctionBuilder<'a>,
    cpu: Value,
    va_base: Value,
    reg: [Option<Variable>; 27],
    s_var: Variable,
    c_var: Variable,
    left: Variable,
    exit: IrBlock,
    exit_pc: Variable,
    exit_unrun: Variable,
    flags_live: bool,
    fcmp: FuncRef,
    real_words: FuncRef,
    words_slot: StackSlot,
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

    /// Write a guest register: into its variable for later reads, and
    /// through to the CPU at once, so exits need not carry it.
    fn set(&mut self, r: i64, v: Value) {
        if r != 0 {
            self.b.def_var(self.reg[idx(r)].expect("used register"), v);
            self.b.ins().store(FL, v, self.cpu, (OFF_REGS + 8 * idx(r)) as i32);
        }
    }

    /// C, kept lazily like S: `raw` is an unwrapped sum whose carry is C
    /// (C = +1 above W_MAX, −1 below −W_MAX, else 0).
    fn set_c(&mut self, raw: Value) {
        self.b.def_var(self.c_var, raw);
    }

    /// The carry trit of the lazily kept C.
    fn carry(&mut self) -> Value {
        let raw = self.b.use_var(self.c_var);
        let over = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, raw, W_MAX);
        let under = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, raw, -W_MAX);
        let o = self.b.ins().uextend(I64, over);
        let u = self.b.ins().uextend(I64, under);
        self.b.ins().isub(o, u)
    }

    /// S and C into the CPU.
    fn s_to_cpu(&mut self) {
        if self.flags_live {
            let sv = self.b.use_var(self.s_var);
            let s = self.sign(sv);
            self.b.ins().istore8(FL, s, self.cpu, (OFF_MODE + mode::S) as i32);
            let c = self.carry();
            self.b.ins().istore8(FL, c, self.cpu, (OFF_MODE + mode::C) as i32);
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

    /// Wrap a sum of a few words into a word. Returns (word, raw) — the
    /// raw sum keeps the carry for `set_c`.
    fn wrap(&mut self, v: Value) -> (Value, Value) {
        let over = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, v, W_MAX);
        let under = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, v, -W_MAX);
        let down = self.b.ins().iadd_imm_s(v, -W_MOD);
        let up = self.b.ins().iadd_imm_s(v, W_MOD);
        let v1 = self.b.ins().select(over, down, v);
        (self.b.ins().select(under, up, v1), v)
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

    /// Switch to a block whose predecessors are all emitted, and seal it
    /// (cheaper SSA construction than sealing everything at the end).
    fn enter(&mut self, block: IrBlock) {
        self.b.switch_to_block(block);
        self.b.seal_block(block);
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

    fn flags_from_cpu(&mut self) {
        if self.flags_live {
            let s = self.b.ins().sload8(I64, FL, self.cpu, (OFF_MODE + mode::S) as i32);
            let c = self.b.ins().sload8(I64, FL, self.cpu, (OFF_MODE + mode::C) as i32);
            let raw = self.b.ins().imul_imm_s(c, W_MAX + 1); // a raw sum with that carry
            self.b.def_var(self.s_var, s);
            self.b.def_var(self.c_var, raw);
        }
    }

    /// The window test for a word access at `va` through its entry in
    /// the window table (`write`: the write half): (covered?, the word's
    /// host address).
    ///
    /// The word index is (va − first centre) / 3: multiplying by 3's
    /// inverse modulo 2^64 gives it exactly for a word centre, and a huge
    /// number for anything misaligned or below the window, so one unsigned
    /// compare checks range and alignment. (A pending page-table write
    /// needs no test: compiled code is only entered without one and leaves
    /// as soon as a store makes one; an address that would need wrapping
    /// is outside every window.)
    fn window(&mut self, va: Value, write: bool) -> (Value, Value) {
        const INV3: i64 = 0xAAAA_AAAA_AAAA_AAABu64 as i64;
        let k = self.b.ins().sshr_imm_s(va, JW_SHIFT);
        let k = self.b.ins().band_imm_s(k, (JW - 1) as i64);
        let k = self.b.ins().ishl_imm_s(k, 4); // 16 bytes an entry
        let entry = self.b.ins().iadd(self.cpu, k);
        let base = (OFF_JWIN + if write { 16 * JW } else { 0 }) as i32;
        let lo = self.b.ins().load(I64, FL, entry, base);
        let ptr = self.b.ins().load(I64, FL, entry, base + 8);
        let off = self.b.ins().isub(va, lo);
        let off = self.b.ins().iadd_imm_s(off, -1); // from the first word's centre
        let q = self.b.ins().imul_imm_s(off, INV3);
        let in_window = self.b.ins().icmp_imm_u(IntCC::UnsignedLessThan, q, PAGE / 3);
        let has = self.b.ins().icmp_imm_s(IntCC::NotEqual, ptr, 0);
        let ok = self.b.ins().band(in_window, has);
        let byte = self.b.ins().ishl_imm_s(q, 3);
        let addr = self.b.ins().iadd(ptr, byte);
        (ok, addr)
    }
}

/// 3^9, and its inverse modulo 2^64 (exact division by multiplying).
const T9: i64 = 19_683;
const T9_INV: i64 = {
    let mut x: u64 = 1;
    let mut i = 0;
    while i < 6 {
        x = x.wrapping_mul(2u64.wrapping_sub((T9 as u64).wrapping_mul(x)));
        i += 1;
    }
    x as i64
};
const _: () = assert!(T9.wrapping_mul(T9_INV) == 1);

impl Gen<'_> {
    /// The window test for a tryte access at `va`: (covered?, the host
    /// address of its word, its place in the word: 0 high, 1, 2 low).
    fn tryte_window(&mut self, va: Value, write: bool) -> (Value, Value, Value) {
        let k = self.b.ins().sshr_imm_s(va, JW_SHIFT);
        let k = self.b.ins().band_imm_s(k, (JW - 1) as i64);
        let k = self.b.ins().ishl_imm_s(k, 4);
        let entry = self.b.ins().iadd(self.cpu, k);
        let base = (OFF_JWIN + if write { 16 * JW } else { 0 }) as i32;
        let lo = self.b.ins().load(I64, FL, entry, base);
        let ptr = self.b.ins().load(I64, FL, entry, base + 8);
        let off = self.b.ins().isub(va, lo);
        let in_page = self.b.ins().icmp_imm_u(IntCC::UnsignedLessThan, off, PAGE);
        let has = self.b.ins().icmp_imm_s(IntCC::NotEqual, ptr, 0);
        let ok = self.b.ins().band(in_page, has);
        // (only used when off < PAGE) q = off / 3, r = off - 3q
        let q = self.b.ins().udiv_imm_u(off, 3);
        let q3 = self.b.ins().imul_imm_s(q, 3);
        let r = self.b.ins().isub(off, q3);
        let byte = self.b.ins().ishl_imm_s(q, 3);
        (ok, self.b.ins().iadd(ptr, byte), r)
    }

    /// The window test for `words` consecutive words from `va` (all in one
    /// window's page; `write`: the write window): (covered?, the first
    /// word's host address).
    fn window_span(&mut self, va: Value, words: i64, write: bool) -> (Value, Value) {
        const INV3: i64 = 0xAAAA_AAAA_AAAA_AAABu64 as i64;
        let k = self.b.ins().sshr_imm_s(va, JW_SHIFT);
        let k = self.b.ins().band_imm_s(k, (JW - 1) as i64);
        let k = self.b.ins().ishl_imm_s(k, 4);
        let entry = self.b.ins().iadd(self.cpu, k);
        let base = (OFF_JWIN + if write { 16 * JW } else { 0 }) as i32;
        let lo = self.b.ins().load(I64, FL, entry, base);
        let ptr = self.b.ins().load(I64, FL, entry, base + 8);
        let off = self.b.ins().isub(va, lo);
        let off = self.b.ins().iadd_imm_s(off, -1);
        let q = self.b.ins().imul_imm_s(off, INV3);
        let in_window = self.b.ins().icmp_imm_u(IntCC::UnsignedLessThan, q, PAGE / 3 - (words - 1));
        let has = self.b.ins().icmp_imm_s(IntCC::NotEqual, ptr, 0);
        let ok = self.b.ins().band(in_window, has);
        let byte = self.b.ins().ishl_imm_s(q, 3);
        (ok, self.b.ins().iadd(ptr, byte))
    }

    /// FMOV, FLDD, FLDS, FSTD and FCMP natively (other FPU instructions:
    /// None). The checks (the FPU enabled; an access's words in a window)
    /// branch to the slow block, where the caller emits the interpreter's
    /// code; returns that block, the one both paths go on to, and for a
    /// load or store its address and which (the slow path then fills the
    /// window table with it).
    fn fpu_inline(&mut self, d: &Dec) -> Option<FpuInline> {
        let (a, f) = (d.a as i64, d.f as i64);
        let fmov = a == op::FCVT && f == fcvt::FMOV;
        let load = (a == op::FMEM || a == op::FMEMD) && (f == 1 || f == 2);
        let store = (a == op::FMEM || a == op::FMEMD) && f == -2; // a real81 (a real27 may trap)
        let fcmp = a == op::FALU && f == falu::FCMP;
        if !(fmov || load || store || fcmp) || ((fmov || load) && d.x == 0) {
            return None; // (f0 is not written)
        }
        let (fast, slow, done) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
        self.b.set_cold_block(slow);
        let fe = self.b.ins().sload8(I64, FL, self.cpu, (OFF_MODE + mode::FE) as i32);
        let mut ok = self.b.ins().icmp_imm_s(IntCC::Equal, fe, 1);
        if fcmp {
            self.b.ins().brif(ok, fast, &[], slow, &[]);
            self.enter(fast);
            let (y, z) = (self.b.ins().iconst(I64, d.y as i64), self.b.ins().iconst(I64, d.z as i64));
            let call = self.b.ins().call(self.fcmp, &[self.cpu, y, z, y]);
            let r = self.b.inst_results(call)[0];
            // S the order (0 when unordered), C 1 when unordered
            let unordered = self.b.ins().icmp_imm_s(IntCC::Equal, r, 2);
            let zero = self.b.ins().iconst(I64, 0);
            let s = self.b.ins().select(unordered, zero, r);
            let carry = self.b.ins().iconst(I64, W_MAX + 1);
            let c = self.b.ins().select(unordered, carry, zero);
            self.b.def_var(self.s_var, s);
            self.b.def_var(self.c_var, c);
            self.b.ins().jump(done, &[]);
            self.b.switch_to_block(slow);
            self.b.seal_block(slow);
            return Some((slow, done, None));
        }
        if store {
            let off = if a == op::FMEM { 9 * d.z as i64 } else { d.imm };
            let base = self.get(d.y as i64);
            let va = self.b.ins().iadd_imm_s(base, off);
            let (inw, addr) = self.window_span(va, 3, true);
            ok = self.b.ins().band(ok, inw);
            self.b.ins().brif(ok, fast, &[], slow, &[]);
            self.enter(fast);
            let x = self.b.ins().iconst(I64, d.x as i64);
            let out = self.b.ins().stack_addr(I64, self.words_slot, 0);
            self.b.ins().call(self.real_words, &[self.cpu, x, out, x]);
            for k in 0..3 {
                let w = self.b.ins().stack_load(I64, I64, self.words_slot, 8 * k);
                self.b.ins().store(FL, w, addr, 8 * k);
            }
            self.b.ins().jump(done, &[]);
            self.b.switch_to_block(slow);
            self.b.seal_block(slow);
            return Some((slow, done, Some((va, true))));
        }
        let dst = (OFF_FREGS + REAL_SIZE * idx(d.x as i64)) as i32;
        let mut load_va = None;
        let (e, m) = if fmov {
            let src = (OFF_FREGS + REAL_SIZE * idx(d.y as i64)) as i32;
            self.b.ins().brif(ok, fast, &[], slow, &[]);
            self.enter(fast);
            let e = self.b.ins().load(I64, FL, self.cpu, src + REAL_E as i32);
            let m = self.b.ins().load(I128, FL, self.cpu, src + REAL_M as i32);
            (e, m)
        } else {
            // the address as exec_fpu forms it (unwrapped: outside every
            // window if it would wrap)
            // the function is the size class: 1 a real27 (one word), 2 a
            // real81 (three)
            let words = if f == 2 { 3 } else { 1 };
            let off = if a == op::FMEM { 3 * words * d.z as i64 } else { d.imm };
            let base = self.get(d.y as i64);
            let va = self.b.ins().iadd_imm_s(base, off);
            load_va = Some((va, false));
            let (inw, addr) = self.window_span(va, words, false);
            ok = self.b.ins().band(ok, inw);
            self.b.ins().brif(ok, fast, &[], slow, &[]);
            self.enter(fast);
            let w0 = self.b.ins().load(I64, FL, addr, 0);
            let zero = self.b.ins().iconst(I64, 0);
            let (w1, w2) = if words == 3 {
                (self.b.ins().load(I64, FL, addr, 8), self.b.ins().load(I64, FL, addr, 16))
            } else {
                (zero, zero)
            };
            // Real::from_words: hi = w0 mod 3^18 (balanced), e = (w0 - hi) / 3^18,
            // m = (hi * 3^27 + w1) * 3^27 + w2
            const T18: i64 = 387_420_489;
            const H18: i64 = (T18 - 1) / 2;
            const T18_INV: i64 = {
                let mut x: u64 = 1;
                let mut i = 0;
                while i < 6 {
                    x = x.wrapping_mul(2u64.wrapping_sub((T18 as u64).wrapping_mul(x)));
                    i += 1;
                }
                x as i64
            };
            let c = self.b.ins().iadd_imm_s(w0, H18 + T18 * (1 << 14));
            let u = self.b.ins().urem_imm_u(c, T18);
            let hi = self.b.ins().iadd_imm_s(u, -H18);
            let top = self.b.ins().isub(w0, hi);
            let e = self.b.ins().imul_imm_s(top, T18_INV);
            let t27 = self.b.ins().iconst(I64, W_MOD);
            let t27 = self.b.ins().sextend(I128, t27);
            let hi = self.b.ins().sextend(I128, hi);
            let w1 = self.b.ins().sextend(I128, w1);
            let w2 = self.b.ins().sextend(I128, w2);
            let m = self.b.ins().imul(hi, t27);
            let m = self.b.ins().iadd(m, w1);
            let m = self.b.ins().imul(m, t27);
            let m = self.b.ins().iadd(m, w2);
            (e, m)
        };
        self.b.ins().store(FL, e, self.cpu, dst + REAL_E as i32);
        self.b.ins().store(FL, m, self.cpu, dst + REAL_M as i32);
        self.b.ins().jump(done, &[]);
        self.b.switch_to_block(slow);
        self.b.seal_block(slow);
        Some((slow, done, load_va))
    }

    /// The low tryte of a word, balanced: v mod 3^9 in -9841..9841.
    fn low_tryte(&mut self, v: Value) -> Value {
        // into the positive range first (|v| < 3^27 / 2 < 3^9 * 2^31)
        let c = self.b.ins().iadd_imm_s(v, 9841 + T9 * (1 << 31));
        let u = self.b.ins().urem_imm_u(c, T9);
        self.b.ins().iadd_imm_s(u, -9841)
    }

    /// The tryte at place `r` (0 high .. 2 low) of word `w`.
    fn tryte_of(&mut self, w: Value, r: Value) -> Value {
        let t2 = self.low_tryte(w);
        let rest = self.b.ins().isub(w, t2);
        let rest = self.b.ins().imul_imm_s(rest, T9_INV); // exact: (w - t2) / 3^9
        let t1 = self.low_tryte(rest);
        let top = self.b.ins().isub(rest, t1);
        let t0 = self.b.ins().imul_imm_s(top, T9_INV);
        let is0 = self.b.ins().icmp_imm_s(IntCC::Equal, r, 0);
        let is1 = self.b.ins().icmp_imm_s(IntCC::Equal, r, 1);
        let t12 = self.b.ins().select(is1, t1, t2);
        self.b.ins().select(is0, t0, t12)
    }
}

impl Jit {
    pub(super) fn new() -> Option<Jit> {
        let mut flags = settings::builder();
        // compiles run in the background: the better code is worth it
        // (mandel 9.9x -> 7.7x the native time; compiling about 30% slower)
        flags.set("opt_level", "speed").ok()?;
        flags.set("use_colocated_libcalls", "false").ok()?;
        flags.set("enable_verifier", "false").ok()?;
        flags.set("is_pic", "false").ok()?;
        let isa = cranelift_native::builder().ok()?.finish(settings::Flags::new(flags)).ok()?;
        let mut jb = JITBuilder::with_isa(isa, default_libcall_names());
        jb.symbol("jit_load_word", jit_load_word as *const u8);
        jb.symbol("jit_store_word", jit_store_word as *const u8);
        jb.symbol("jit_load_tryte", jit_load_tryte as *const u8);
        jb.symbol("jit_store_tryte", jit_store_tryte as *const u8);
        jb.symbol("jit_exec_one", jit_exec_one as *const u8);
        jb.symbol("jit_alu", jit_alu as *const u8);
        jb.symbol("jit_fcmp", jit_fcmp as *const u8);
        jb.symbol("jit_real_words", jit_real_words as *const u8);
        jb.symbol("jit_fill_window", jit_fill_window as *const u8);
        let mut module = JITModule::new(jb);
        let ptr = module.target_config().pointer_type();
        let mut decl = |name: &str, params: &[cranelift_codegen::ir::Type]| -> Option<FuncId> {
            let mut sig = module.make_signature();
            for &t in params {
                sig.params.push(AbiParam::new(t));
            }
            sig.returns.push(AbiParam::new(I64));
            module.declare_function(name, Linkage::Import, &sig).ok()
        };
        let load_word = decl("jit_load_word", &[ptr, ptr, I64, ptr])?;
        let store_word = decl("jit_store_word", &[ptr, ptr, I64, I64])?;
        let load_tryte = decl("jit_load_tryte", &[ptr, ptr, I64, ptr])?;
        let store_tryte = decl("jit_store_tryte", &[ptr, ptr, I64, I64])?;
        let exec_one = decl("jit_exec_one", &[ptr, ptr, ptr, I64])?;
        let alu = decl("jit_alu", &[I64, I64, I64])?;
        let fcmp = decl("jit_fcmp", &[ptr, I64, I64, I64])?;
        let real_words = decl("jit_real_words", &[ptr, I64, ptr, I64])?;
        let fill_window = decl("jit_fill_window", &[ptr, I64, I64, I64])?;
        Some(Jit {
            ctx: module.make_context(),
            module,
            fctx: FunctionBuilderContext::new(),
            load_word,
            store_word,
            load_tryte,
            store_tryte,
            exec_one,
            alu,
            fcmp,
            real_words,
            fill_window,
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
        for t in [ptr, ptr, I64, I64, I64] {
            self.ctx.func.signature.params.push(AbiParam::new(t));
        }
        self.ctx.func.signature.returns.push(AbiParam::new(I64));
        let name = format!("region{}", self.next_id);
        self.next_id += 1;
        let fid = self.module.declare_function(&name, Linkage::Local, &self.ctx.func.signature).ok()?;
        self.ctx.func.name = UserFuncName::user(0, fid.as_u32());

        let (load_word, store_word, exec_one) = (self.load_word, self.store_word, self.exec_one);
        let (load_tryte, store_tryte) = (self.load_tryte, self.store_tryte);
        let mut b = FunctionBuilder::new(&mut self.ctx.func, &mut self.fctx);
        let lw = self.module.declare_func_in_func(load_word, b.func);
        let sw = self.module.declare_func_in_func(store_word, b.func);
        let lt = self.module.declare_func_in_func(load_tryte, b.func);
        let st = self.module.declare_func_in_func(store_tryte, b.func);
        let ex = self.module.declare_func_in_func(exec_one, b.func);
        let alu_f = self.module.declare_func_in_func(self.alu, b.func);
        let fcmp = self.module.declare_func_in_func(self.fcmp, b.func);
        let real_words = self.module.declare_func_in_func(self.real_words, b.func);
        let fill = self.module.declare_func_in_func(self.fill_window, b.func);

        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let p = b.block_params(entry).to_vec();
        let (cpu, mem, va_base, budget, entry_off) = (p[0], p[1], p[2], p[3], p[4]);
        let slot = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
        let words_slot = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 24, 3));

        // which registers the region uses
        let mut used = [false; 27];
        let mut flags_live = false;
        for rb in region {
            for d in &rb.ops {
                for r in [d.x, d.y, d.z] {
                    used[idx(r as i64)] = true;
                }
                flags_live |= !matches!(d.op, Op::Li | Op::Lea | Op::Jal | Op::Jalr | Op::Ldw | Op::Stw | Op::Generic);
            }
        }
        used[idx(0)] = false;

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
        let mut g = Gen { b, cpu, va_base, reg, s_var, c_var, left, exit, exit_pc, exit_unrun, flags_live, fcmp, real_words, words_slot };
        g.flags_from_cpu();

        let mut blocks = HashMap::new();
        for rb in region {
            blocks.insert(rb.off, g.b.create_block());
        }
        // enter at any of its blocks (a batch may end anywhere in a loop);
        // an offset that is not one of them runs nothing
        if region.len() == 1 {
            let first = blocks[&region[0].off];
            g.b.ins().jump(first, &[]);
        } else {
            let none = g.b.create_block();
            let mut sw = Switch::new();
            for rb in region {
                sw.set_entry((rb.off + 3 * PAGE) as u128, blocks[&rb.off]);
            }
            let key = g.b.ins().iadd_imm_s(entry_off, 3 * PAGE);
            sw.emit(&mut g.b, key, none);
            g.enter(none);
            let pc = g.b.ins().iadd(va_base, entry_off);
            g.leave(pc, 0);
        }

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
            g.enter(stop);
            let pc = g.pc_in_page(rb.off);
            g.leave(pc, 0);
            g.enter(go);
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
                        g.set_c(carry);
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
                            let raw = g.b.ins().imul_imm_s(carry, W_MAX + 1);
                            g.set_c(raw);
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
                        g.enter(fault);
                        let pc = g.pc_in_page(off);
                        let keep = Box::new(*d);
                        let dp = g.b.ins().iconst(ptr, &*keep as *const Dec as i64);
                        ops_keep.push(keep);
                        g.s_to_cpu();
                        g.b.ins().call(ex, &[cpu, mem, dp, pc]);
                        g.leave(pc, unrun);
                        g.enter(divide);
                        let r = if d.f as i64 == alu::DIV { g.b.ins().sdiv(a, c) } else { g.b.ins().srem(a, c) };
                        g.b.def_var(g.s_var, r);
                        g.set(x, r);
                    }
                    Op::AluR | Op::AluK if matches!(d.f as i64, alu::BAND | alu::BOR | alu::BXOR | alu::BANDN) => {
                        // an i64 in two's complement is the unlimited-width
                        // pattern; results stay within 2^42, so one wrap does
                        let a = g.get(y);
                        let c = if d.op == Op::AluR { g.get(z) } else { g.b.ins().iconst(I64, d.imm) };
                        let v = match d.f as i64 {
                            alu::BAND => g.b.ins().band(a, c),
                            alu::BOR => g.b.ins().bor(a, c),
                            alu::BXOR => g.b.ins().bxor(a, c),
                            _ => g.b.ins().band_not(a, c),
                        };
                        let r = g.wrap(v).0;
                        g.b.def_var(g.s_var, r);
                        g.set(x, r);
                    }
                    Op::Bfld => {
                        let a = g.get(y);
                        let (pos, len) = (d.imm, d.imm2 as i64);
                        let mask = (1i64 << len) - 1;
                        let r = if d.f as i64 == crate::isa::fld::BEXT {
                            let s = g.b.ins().sshr_imm_s(a, pos.min(63));
                            g.b.ins().band_imm_s(s, mask)
                        } else {
                            let v = g.b.ins().band_imm_s(a, mask);
                            let l = g.b.ins().ishl_imm_s(v, pos);
                            let h = g.b.ins().ushr_imm_s(v, len - pos);
                            let o = g.b.ins().bor(l, h);
                            g.b.ins().band_imm_s(o, mask)
                        };
                        g.b.def_var(g.s_var, r);
                        g.set(x, r);
                    }
                    Op::AluR | Op::AluK if alu_by_helper(d.f as i64) => {
                        let a = g.get(y);
                        let c = if d.op == Op::AluR { g.get(z) } else { g.b.ins().iconst(I64, d.imm) };
                        let f = g.b.ins().iconst(I64, d.f as i64);
                        let call = g.b.ins().call(alu_f, &[f, a, c]);
                        let r = g.b.inst_results(call)[0];
                        g.b.def_var(g.s_var, r);
                        g.set(x, r);
                    }
                    Op::AluR | Op::AluK if matches!(d.f as i64, alu::NEG | alu::SGN | alu::ABS) => {
                        let a = g.get(y);
                        let r = match d.f as i64 {
                            alu::NEG => g.b.ins().ineg(a),
                            alu::SGN => g.sign(a),
                            _ => g.b.ins().iabs(a),
                        };
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
                        // unwrapped: a miss if it would need wrapping, and
                        // the helper wraps it
                        let va = g.b.ins().iadd_imm_s(base, d.imm);
                        let fast = g.b.create_block();
                        let slow = g.b.create_block();
                        let done = g.b.create_block();
                        let (ok, addr) = g.window(va, d.op == Op::Stw);
                        g.b.set_cold_block(slow);
                        g.b.ins().brif(ok, fast, &[], slow, &[]);
                        // through the window
                        g.enter(fast);
                        if d.op == Op::Ldw {
                            let v = g.b.ins().load(I64, FL, addr, 0);
                            g.set(x, v);
                        } else {
                            let v = g.get(x);
                            g.b.ins().store(FL, v, addr, 0);
                        }
                        g.b.ins().jump(done, &[]);
                        // the helper: TLB, a new window, or a fault
                        g.enter(slow);
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
                        g.enter(fault);
                        let pc = g.pc_in_page(off);
                        g.leave(pc, unrun);
                        g.enter(ok);
                        if d.op == Op::Ldw {
                            let v = g.b.ins().stack_load(ptr, I64, slot, 0);
                            g.set(x, v);
                            g.b.ins().jump(done, &[]);
                        } else {
                            // it wrote code or a page table: leave after it
                            let out = g.b.create_block();
                            let dirty = g.b.ins().icmp_imm_s(IntCC::Equal, status, OK_EXIT);
                            g.b.ins().brif(dirty, out, &[], done, &[]);
                            g.enter(out);
                            let pc = g.pc_in_page(next_off);
                            g.leave(pc, unrun - 1);
                        }
                        g.enter(done);
                    }
                    Op::Ldt | Op::Stt => {
                        let base = g.get(y);
                        let va = g.b.ins().iadd_imm_s(base, d.imm);
                        let fast = g.b.create_block();
                        let slow = g.b.create_block();
                        let done = g.b.create_block();
                        let (ok, addr, r) = g.tryte_window(va, d.op == Op::Stt);
                        g.b.set_cold_block(slow);
                        g.b.ins().brif(ok, fast, &[], slow, &[]);
                        // through the window: the word, and its tryte r
                        g.enter(fast);
                        let w = g.b.ins().load(I64, FL, addr, 0);
                        let old = g.tryte_of(w, r);
                        if d.op == Op::Ldt {
                            g.set(x, old);
                        } else {
                            // w + (new - old) * 3^(9 * (2 - r))
                            let v = g.get(x);
                            let new = g.low_tryte(v);
                            let is0 = g.b.ins().icmp_imm_s(IntCC::Equal, r, 0);
                            let is1 = g.b.ins().icmp_imm_s(IntCC::Equal, r, 1);
                            let w0 = g.b.ins().iconst(I64, T9 * T9);
                            let w1 = g.b.ins().iconst(I64, T9);
                            let w2 = g.b.ins().iconst(I64, 1);
                            let w12 = g.b.ins().select(is1, w1, w2);
                            let weight = g.b.ins().select(is0, w0, w12);
                            let diff = g.b.ins().isub(new, old);
                            let delta = g.b.ins().imul(diff, weight);
                            let nw = g.b.ins().iadd(w, delta);
                            g.b.ins().store(FL, nw, addr, 0);
                        }
                        g.b.ins().jump(done, &[]);
                        // the helper: TLB, a new window, or a fault
                        g.enter(slow);
                        let status = if d.op == Op::Ldt {
                            let out = g.b.ins().stack_addr(ptr, slot, 0);
                            let call = g.b.ins().call(lt, &[cpu, mem, va, out]);
                            g.b.inst_results(call)[0]
                        } else {
                            let v = g.get(x);
                            let call = g.b.ins().call(st, &[cpu, mem, va, v]);
                            g.b.inst_results(call)[0]
                        };
                        let fault = g.b.create_block();
                        let ok = g.b.create_block();
                        g.b.set_cold_block(fault);
                        let bad = g.b.ins().icmp_imm_s(IntCC::Equal, status, FAULT);
                        g.b.ins().brif(bad, fault, &[], ok, &[]);
                        g.enter(fault);
                        let pc = g.pc_in_page(off);
                        g.leave(pc, unrun);
                        g.enter(ok);
                        if d.op == Op::Ldt {
                            let v = g.b.ins().stack_load(ptr, I64, slot, 0);
                            g.set(x, v);
                            g.b.ins().jump(done, &[]);
                        } else {
                            let out = g.b.create_block();
                            let dirty = g.b.ins().icmp_imm_s(IntCC::Equal, status, OK_EXIT);
                            g.b.ins().brif(dirty, out, &[], done, &[]);
                            g.enter(out);
                            let pc = g.pc_in_page(next_off);
                            g.leave(pc, unrun - 1);
                        }
                        g.enter(done);
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
                                        let c = g.carry();
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
                            g.enter(yes);
                            g.goto(&blocks, t);
                            g.enter(no);
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
                        // FMOV / FLDD / FLDS natively when they can be; the
                        // interpreter's code below is then their slow path
                        let inline = if d.op == Op::Fpu { g.fpu_inline(d) } else { None };
                        // the interpreter's code for this one instruction
                        // (registers and C are already in the CPU)
                        g.s_to_cpu();
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
                        g.enter(fault);
                        g.leave(pc, unrun);
                        g.enter(ok);
                        if x != 0 {
                            let v = g.b.ins().load(I64, FL, cpu, (OFF_REGS + 8 * idx(x)) as i32);
                            g.b.def_var(g.reg[idx(x)].expect("used register"), v);
                        }
                        g.flags_from_cpu();
                        let out = g.b.create_block();
                        let on = g.b.create_block();
                        let dirty = g.b.ins().icmp_imm_s(IntCC::Equal, status, OK_EXIT);
                        g.b.ins().brif(dirty, out, &[], on, &[]);
                        g.enter(out);
                        let pc = g.pc_in_page(next_off);
                        g.leave(pc, unrun - 1);
                        g.enter(on);
                        if let Some((_, done, va)) = inline {
                            if let Some((va, write)) = va {
                                // the access went through: its page into the
                                // window table, for the native path next time
                                let w = g.b.ins().iconst(I64, write as i64);
                                g.b.ins().call(fill, &[cpu, va, w, w]);
                            }
                            g.b.ins().jump(done, &[]);
                            g.enter(done);
                        }
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
        // registers are written through; S and C are kept lazily
        g.s_to_cpu();
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
    /// The request queue all compiler threads take from: whichever is free
    /// takes the next region, so a long compile holds up nothing else.
    tx: std::sync::mpsc::Sender<Request>,
    rx: std::sync::mpsc::Receiver<Done>,
}

/// Compiler threads: regions compile in parallel, so hot code waits less
/// (all cores but the emulator's own, at most 8).
fn jit_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    cores.saturating_sub(1).clamp(1, 8)
}

impl JitThread {
    pub(super) fn spawn() -> Option<JitThread> {
        let (done_tx, rx) = std::sync::mpsc::channel::<Done>();
        let (tx, req_rx) = std::sync::mpsc::channel::<Request>();
        let req_rx = std::sync::Arc::new(std::sync::Mutex::new(req_rx));
        for k in 0..jit_threads() {
            let req_rx = req_rx.clone();
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
                    loop {
                        // (the lock is held only while waiting for a request)
                        let Ok(req) = req_rx.lock().map_err(|_| ()).and_then(|r| r.recv().map_err(|_| ())) else { break };
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
        }
        Some(JitThread { tx, rx })
    }

    pub(super) fn send(&self, r: Request) -> bool {
        self.tx.send(r).is_ok()
    }

    pub(super) fn try_recv(&self) -> Option<Done> {
        self.rx.try_recv().ok()
    }

    pub(super) fn recv(&self) -> Option<Done> {
        self.rx.recv().ok()
    }
}
