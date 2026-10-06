//! The JIT of the WebAssembly build: hot regions compiled to small wasm
//! modules, which the page compiles and links (the module cannot compile
//! code itself).
//!
//! The regions and their semantics are the native JIT's (`jit.rs`); only
//! the code differs. A region becomes one function
//! `run(cpu, mem, va_base, budget, entry_off) -> steps`, in a module that
//! imports the emulator's memory and function table. Guest registers are
//! wasm locals (written through to the CPU), S and C are kept lazily, and
//! loads and stores go through the CPU's windows. The helpers (`jit_rt.rs`)
//! are called through the table: in wasm a function pointer is its index
//! there. Jumps within the region go round a dispatch loop
//! (`br_table` on the block's number), since wasm has no goto.
//!
//! The page takes each module's bytes (`take_request`), compiles it in the
//! background, adds its `run` to the table and reports the index
//! (`finished`); the CPU installs the code at its next look, as with the
//! native compiler threads.

use super::jit_rt::*;
use super::*;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

thread_local! {
    /// Modules waiting for the page: (request id, bytes).
    static OUTBOX: RefCell<VecDeque<(u64, Vec<u8>)>> = const { RefCell::new(VecDeque::new()) };
    /// Compiled ones: (request id, table index, or 0 if it failed).
    static INBOX: RefCell<VecDeque<(u64, u32)>> = const { RefCell::new(VecDeque::new()) };
}

thread_local! {
    /// Each JIT (one per CPU) tags its requests' ids with its own number,
    /// so a module still compiling for a machine that is gone is never
    /// installed in its successor (request ids start again from 0).
    static GENERATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

const GEN_SHIFT: u32 = 40;

/// The next module for the page to compile.
pub(super) fn take_request() -> Option<(u64, Vec<u8>)> {
    OUTBOX.with(|o| o.borrow_mut().pop_front())
}

#[cfg(feature = "wasm-sync-jit")]
extern "C" {
    /// The host compiles and links every module waiting, before returning.
    fn setun_jit_service();
}

/// The page compiled request `id`: its `run` is at `index` in the table
/// (0: it failed).
pub(super) fn finished(id: u64, index: u32) {
    INBOX.with(|i| i.borrow_mut().push_back((id, index)));
}

pub(super) struct JitThread {
    /// Decoded instructions the code points at (for the interpreter's
    /// fallback), kept as long as the code may run (boxed: at fixed
    /// addresses).
    #[allow(clippy::vec_box)]
    keep: RefCell<Vec<Box<Dec>>>,
    /// This JIT's number, in the high bits of the ids the page sees.
    generation: u64,
}

impl JitThread {
    pub(super) fn spawn() -> Option<JitThread> {
        let generation = GENERATIONS.with(|g| {
            g.set(g.get() + 1);
            g.get()
        });
        Some(JitThread { keep: RefCell::new(Vec::new()), generation })
    }

    pub(super) fn send(&self, r: Request) -> bool {
        let bytes = module(&r.region, &mut self.keep.borrow_mut());
        OUTBOX.with(|o| o.borrow_mut().push_back((self.generation << GEN_SHIFT | r.id, bytes)));
        #[cfg(feature = "wasm-sync-jit")]
        unsafe {
            setun_jit_service();
        }
        true
    }

    pub(super) fn try_recv(&self) -> Option<Done> {
        INBOX.with(|i| {
            let mut i = i.borrow_mut();
            // those of an earlier machine are dropped
            while let Some((id, idx)) = i.pop_front() {
                if id >> GEN_SHIFT == self.generation {
                    return Some(Done { id: id & ((1 << GEN_SHIFT) - 1), code: idx as usize, micros: 0 });
                }
            }
            None
        })
    }

    /// (No waiting here: the page compiles between runs.)
    pub(super) fn recv(&self) -> Option<Done> {
        self.try_recv()
    }
}

// ---- the binary format --------------------------------------------------------------

fn leb_u(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn leb_s(out: &mut Vec<u8>, mut v: i64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        let done = (v == 0 && b & 0x40 == 0) || (v == -1 && b & 0x40 != 0);
        out.push(if done { b } else { b | 0x80 });
        if done {
            return;
        }
    }
}

fn section(out: &mut Vec<u8>, id: u8, body: &[u8]) {
    out.push(id);
    leb_u(out, body.len() as u64);
    out.extend_from_slice(body);
}

fn name(out: &mut Vec<u8>, s: &str) {
    leb_u(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

const I32: u8 = 0x7f;
const I64T: u8 = 0x7e;

// opcodes
const BLOCK: u8 = 0x02;
const LOOP: u8 = 0x03;
const IF: u8 = 0x04;
const ELSE: u8 = 0x05;
const END: u8 = 0x0b;
const BR: u8 = 0x0c;
const BR_TABLE: u8 = 0x0e;
const RETURN: u8 = 0x0f;
const CALL_INDIRECT: u8 = 0x11;
const SELECT: u8 = 0x1b;
const LGET: u8 = 0x20;
const LSET: u8 = 0x21;
const LTEE: u8 = 0x22;
const I64_LOAD: u8 = 0x29;
const I64_LOAD8_S: u8 = 0x30;
const I64_STORE: u8 = 0x37;
const I64_STORE8: u8 = 0x3c;
const I32_CONST: u8 = 0x41;
const I64_CONST: u8 = 0x42;
const I32_AND: u8 = 0x71;
const I32_OR: u8 = 0x72;
const I32_ADD: u8 = 0x6a;
const I64_EQZ: u8 = 0x50;
const I64_EQ: u8 = 0x51;
const I64_NE: u8 = 0x52;
const I64_LT_S: u8 = 0x53;
const I64_LT_U: u8 = 0x54;
const I64_GT_S: u8 = 0x55;
const I64_ADD: u8 = 0x7c;
const I64_SUB: u8 = 0x7d;
const I64_MUL: u8 = 0x7e;
const I64_DIV_S: u8 = 0x7f;
const I64_DIV_U: u8 = 0x80;
const I64_REM_S: u8 = 0x81;
const I64_REM_U: u8 = 0x82;
const I64_AND: u8 = 0x83;
const I64_SHL: u8 = 0x86;
const I64_SHR_S: u8 = 0x87;
const I32_WRAP_I64: u8 = 0xa7;
const I64_EXTEND_I32_U: u8 = 0xad;
const F64_MUL: u8 = 0xa2;
const F64_DIV: u8 = 0xa3;
const F64_NEAREST: u8 = 0x9e;
const F64_CONST: u8 = 0x44;
const F64_CONVERT_I64_S: u8 = 0xb9;
const PREFIX_FC: u8 = 0xfc;
const I64_TRUNC_SAT_F64_S: u8 = 0x06;

// function types
const T_RUN: u32 = 0; // (cpu, mem, va_base, budget, entry_off) -> steps
const T_LOAD: u32 = 1; // (cpu, mem, va, out) -> status
const T_STORE: u32 = 2; // (cpu, mem, va, v) -> status
const T_EXEC: u32 = 3; // (cpu, mem, dec, pc) -> status
const T_ALU: u32 = 4; // (f, x, y) -> r
const T_FILL: u32 = 5; // (cpu, va, write, _) -> status

/// The module for one region.
#[allow(clippy::vec_box)]
pub(super) fn module(region: &[RegionBlock], keep: &mut Vec<Box<Dec>>) -> Vec<u8> {
    let body = function(region, keep);
    let mut m = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    // types
    let mut t = Vec::new();
    let sigs: [(&[u8], u8); 6] = [
        (&[I32, I32, I64T, I64T, I64T], I64T),
        (&[I32, I32, I64T, I32], I64T),
        (&[I32, I32, I64T, I64T], I64T),
        (&[I32, I32, I32, I64T], I64T),
        (&[I64T, I64T, I64T], I64T),
        (&[I32, I64T, I64T, I64T], I64T),
    ];
    leb_u(&mut t, sigs.len() as u64);
    for (params, ret) in sigs {
        t.push(0x60);
        leb_u(&mut t, params.len() as u64);
        t.extend_from_slice(params);
        t.extend_from_slice(&[1, ret]);
    }
    section(&mut m, 1, &t);
    // imports: the emulator's memory and function table
    let mut i = Vec::new();
    leb_u(&mut i, 2);
    name(&mut i, "env");
    name(&mut i, "memory");
    i.extend_from_slice(&[0x02, 0x00, 0x00]); // memory, min 0
    name(&mut i, "env");
    name(&mut i, "table");
    i.extend_from_slice(&[0x01, 0x70, 0x00, 0x00]); // funcref table, min 0
    section(&mut m, 2, &i);
    section(&mut m, 3, &[1, T_RUN as u8]);
    let mut e = Vec::new();
    leb_u(&mut e, 1);
    name(&mut e, "run");
    e.extend_from_slice(&[0x00, 0x00]); // function 0
    section(&mut m, 7, &e);
    let mut c = Vec::new();
    leb_u(&mut c, 1);
    leb_u(&mut c, body.len() as u64);
    c.extend_from_slice(&body);
    section(&mut m, 10, &c);
    m
}

// ---- code generation ----------------------------------------------------------------

/// Locals: the parameters, then the guest registers in use, then these.
struct Locals {
    reg: [u32; 27],
    s: u32,
    c: u32,
    left: u32,
    exit_pc: u32,
    exit_unrun: u32,
    t: [u32; 8],
    next: u32,
}

const P_CPU: u32 = 0;
const P_MEM: u32 = 1;
const P_VA: u32 = 2;
const P_BUDGET: u32 = 3;
const P_ENTRY: u32 = 4;

struct Gen {
    c: Vec<u8>,
    l: Locals,
    /// Blocks in the region, and the one being emitted.
    n: u32,
    k: u32,
    /// Constructs opened inside the current block's code.
    extra: u32,
    flags_live: bool,
}

/// 3^9 and its inverse modulo 2^64; 3's inverse.
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
const INV3: i64 = 0xAAAA_AAAA_AAAA_AAABu64 as i64;

impl Gen {
    fn op(&mut self, o: u8) {
        self.c.push(o);
    }
    fn get(&mut self, l: u32) {
        self.c.push(LGET);
        leb_u(&mut self.c, l as u64);
    }
    fn set(&mut self, l: u32) {
        self.c.push(LSET);
        leb_u(&mut self.c, l as u64);
    }
    fn tee(&mut self, l: u32) {
        self.c.push(LTEE);
        leb_u(&mut self.c, l as u64);
    }
    fn i64c(&mut self, v: i64) {
        self.c.push(I64_CONST);
        leb_s(&mut self.c, v);
    }
    fn i32c(&mut self, v: i64) {
        self.c.push(I32_CONST);
        leb_s(&mut self.c, v);
    }
    /// A load or store at offset `off` from the address on the stack.
    fn mem(&mut self, o: u8, align: u32, off: usize) {
        self.c.push(o);
        leb_u(&mut self.c, align as u64);
        leb_u(&mut self.c, off as u64);
    }
    /// Open a block construct (`IF`, `BLOCK`) with no result.
    fn open(&mut self, o: u8) {
        self.c.extend_from_slice(&[o, 0x40]);
        self.extra += 1;
    }
    fn else_(&mut self) {
        self.c.push(ELSE);
    }
    fn end(&mut self) {
        self.c.push(END);
        self.extra -= 1;
    }
    fn br(&mut self, o: u8, depth: u32) {
        self.c.push(o);
        leb_u(&mut self.c, depth as u64);
    }
    fn depth_loop(&self) -> u32 {
        self.n - self.k + self.extra
    }
    fn depth_exit(&self) -> u32 {
        self.depth_loop() + 1
    }
    /// Call helper `f` (its table index) of type `ty`, arguments pushed.
    fn call(&mut self, ty: u32, f: usize) {
        self.i32c(f as i64);
        self.c.push(CALL_INDIRECT);
        leb_u(&mut self.c, ty as u64);
        self.c.push(0);
    }

    /// Push guest register `r`.
    fn reg(&mut self, r: i64) {
        if r == 0 {
            self.i64c(0);
        } else {
            self.get(self.l.reg[idx(r)]);
        }
    }

    /// Guest register `r` := local `t`, and through to the CPU.
    fn set_reg(&mut self, r: i64, t: u32) {
        if r != 0 {
            self.get(t);
            self.set(self.l.reg[idx(r)]);
            self.get(P_CPU);
            self.get(t);
            self.mem(I64_STORE, 3, OFF_REGS + 8 * idx(r));
        }
    }

    /// −1, 0 or +1 of the i64 on the stack.
    fn sign(&mut self) {
        let t = self.l.t[7];
        self.tee(t);
        self.i64c(0);
        self.op(I64_GT_S);
        self.op(I64_EXTEND_I32_U);
        self.get(t);
        self.i64c(0);
        self.op(I64_LT_S);
        self.op(I64_EXTEND_I32_U);
        self.op(I64_SUB);
    }

    /// The carry trit of the lazily kept C.
    fn carry(&mut self) {
        self.get(self.l.c);
        self.i64c(W_MAX);
        self.op(I64_GT_S);
        self.op(I64_EXTEND_I32_U);
        self.get(self.l.c);
        self.i64c(-W_MAX);
        self.op(I64_LT_S);
        self.op(I64_EXTEND_I32_U);
        self.op(I64_SUB);
    }

    fn s_to_cpu(&mut self) {
        if self.flags_live {
            self.get(P_CPU);
            self.get(self.l.s);
            self.sign();
            self.mem(I64_STORE8, 0, OFF_MODE + mode::S);
            self.get(P_CPU);
            self.carry();
            self.mem(I64_STORE8, 0, OFF_MODE + mode::C);
        }
    }

    fn flags_from_cpu(&mut self) {
        if self.flags_live {
            self.get(P_CPU);
            self.mem(I64_LOAD8_S, 0, OFF_MODE + mode::S);
            self.set(self.l.s);
            self.get(P_CPU);
            self.mem(I64_LOAD8_S, 0, OFF_MODE + mode::C);
            self.i64c(W_MAX + 1);
            self.op(I64_MUL);
            self.set(self.l.c);
        }
    }

    /// Wrap the sum in local `t` into a word (in place).
    fn wrap(&mut self, t: u32) {
        // t = t > W_MAX ? t - W_MOD : t; t = t < -W_MAX ? t + W_MOD : t
        self.get(t);
        self.i64c(W_MOD);
        self.op(I64_SUB);
        self.get(t);
        self.get(t);
        self.i64c(W_MAX);
        self.op(I64_GT_S);
        self.op(SELECT);
        self.set(t);
        self.get(t);
        self.i64c(W_MOD);
        self.op(I64_ADD);
        self.get(t);
        self.get(t);
        self.i64c(-W_MAX);
        self.op(I64_LT_S);
        self.op(SELECT);
        self.set(t);
    }

    /// Local `t` := the address `off` from the region's start, wrapped.
    fn pc_any(&mut self, off: i64, t: u32) {
        self.get(P_VA);
        self.i64c(off);
        self.op(I64_ADD);
        self.set(t);
        self.wrap(t);
    }

    /// Leave with the PC in local `t` and `unrun` steps not taken.
    fn leave_t(&mut self, t: u32, unrun: i64) {
        self.get(t);
        self.set(self.l.exit_pc);
        self.i64c(unrun);
        self.set(self.l.exit_unrun);
        let d = self.depth_exit();
        self.br(BR, d);
    }

    /// Leave at the instruction `off` from the start (within the page).
    fn leave_at(&mut self, off: i64, unrun: i64) {
        self.get(P_VA);
        self.i64c(off);
        self.op(I64_ADD);
        self.set(self.l.exit_pc);
        self.i64c(unrun);
        self.set(self.l.exit_unrun);
        let d = self.depth_exit();
        self.br(BR, d);
    }

    /// Go to the region's block at `off`, or leave for it.
    fn goto(&mut self, blocks: &HashMap<i64, u32>, off: i64) {
        match blocks.get(&off) {
            Some(&b) => {
                self.i32c(b as i64);
                self.set(self.l.next);
                let d = self.depth_loop();
                self.br(BR, d);
            }
            None => {
                let t = self.l.t[6];
                self.pc_any(off, t);
                self.leave_t(t, 0);
            }
        }
    }

    /// The low tryte of the i64 in local `t`, balanced, onto the stack.
    fn low_tryte(&mut self, t: u32) {
        self.get(t);
        self.i64c(9841 + T9 * (1 << 31));
        self.op(I64_ADD);
        self.i64c(T9);
        self.op(I64_REM_U);
        self.i64c(-9841);
        self.op(I64_ADD);
    }

    /// The window test for `va` (local): leaves `ok` (i32) on the stack and
    /// the host address in local `addr` (`tryte`: also the place in local
    /// `place`, and any offset in the page counts).
    fn window(&mut self, va: u32, write: bool, tryte: bool, addr: u32, place: u32) {
        let (lo, ptr, q) = (self.l.t[3], self.l.t[4], self.l.t[5]);
        // the entry: cpu + ((va >> JW_SHIFT) & (JW - 1)) * 16
        let base = OFF_JWIN + if write { 16 * JW } else { 0 };
        self.get(P_CPU);
        self.get(va);
        self.i64c(JW_SHIFT);
        self.op(I64_SHR_S);
        self.i64c((JW - 1) as i64);
        self.op(I64_AND);
        self.i64c(4);
        self.op(I64_SHL);
        self.op(I32_WRAP_I64);
        self.op(I32_ADD);
        let entry = self.l.next; // (free here: set again before any use)
        self.set(entry);
        self.get(entry);
        self.mem(I64_LOAD, 3, base);
        self.set(lo);
        self.get(entry);
        self.mem(I64_LOAD, 3, base + 8);
        self.set(ptr);
        // off = va - lo
        self.get(va);
        self.get(lo);
        self.op(I64_SUB);
        if tryte {
            self.set(q);
            // q = off / 3, place = off - 3q, addr = ptr + 8q; ok: off < PAGE
            self.get(q);
            self.i64c(3);
            self.op(I64_DIV_U);
            self.set(lo);
            self.get(q);
            self.get(lo);
            self.i64c(3);
            self.op(I64_MUL);
            self.op(I64_SUB);
            self.set(place);
            self.get(ptr);
            self.get(lo);
            self.i64c(3);
            self.op(I64_SHL);
            self.op(I64_ADD);
            self.set(addr);
            self.get(q);
            self.i64c(PAGE);
            self.op(I64_LT_U);
        } else {
            // q = (off - 1) * 3^-1: the word index for a word centre, huge otherwise
            self.i64c(-1);
            self.op(I64_ADD);
            self.i64c(INV3);
            self.op(I64_MUL);
            self.tee(q);
            self.i64c(3);
            self.op(I64_SHL);
            self.get(ptr);
            self.op(I64_ADD);
            self.set(addr);
            self.get(q);
            self.i64c(PAGE / 3);
            self.op(I64_LT_U);
        }
        self.get(ptr);
        self.i64c(0);
        self.op(I64_NE);
        self.op(I32_AND);
    }

    /// The interpreter's code for instruction `d` at `off`.
    #[allow(clippy::vec_box)]
    fn exec_one(&mut self, d: &Dec, off: i64, next_off: i64, unrun: i64, keep: &mut Vec<Box<Dec>>) {
        self.s_to_cpu();
        let b = Box::new(*d);
        let dp = &*b as *const Dec as usize;
        keep.push(b);
        let st = self.l.t[0];
        self.get(P_CPU);
        self.get(P_MEM);
        self.i32c(dp as i64);
        self.get(P_VA);
        self.i64c(off);
        self.op(I64_ADD);
        self.call(T_EXEC, jit_exec_one as *const () as usize);
        self.set(st);
        self.status_fault(st, off, unrun);
        let x = d.x as i64;
        if x != 0 {
            let r = self.l.reg[idx(x)];
            self.get(P_CPU);
            self.mem(I64_LOAD, 3, OFF_REGS + 8 * idx(x));
            self.set(r);
        }
        self.flags_from_cpu();
        self.status_dirty(st, next_off, unrun);
    }

    /// Status FAULT in local `st`: leave at the instruction.
    fn status_fault(&mut self, st: u32, off: i64, unrun: i64) {
        self.get(st);
        self.i64c(FAULT);
        self.op(I64_EQ);
        self.open(IF);
        self.leave_at(off, unrun);
        self.end();
    }

    /// Status OK_EXIT in local `st` (code or a page table written): leave
    /// after the instruction.
    fn status_dirty(&mut self, st: u32, next_off: i64, unrun: i64) {
        self.get(st);
        self.i64c(OK_EXIT);
        self.op(I64_EQ);
        self.open(IF);
        self.leave_at(next_off, unrun - 1);
        self.end();
    }
}

/// The body of `run` for a region.
#[allow(clippy::vec_box)]
fn function(region: &[RegionBlock], keep: &mut Vec<Box<Dec>>) -> Vec<u8> {
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
    let mut nl = 5u32;
    let mut reg = [u32::MAX; 27];
    for r in 0..27 {
        if used[r] {
            reg[r] = nl;
            nl += 1;
        }
    }
    let first = nl;
    let (s, c, left, exit_pc, exit_unrun) = (first, first + 1, first + 2, first + 3, first + 4);
    let t = [first + 5, first + 6, first + 7, first + 8, first + 9, first + 10, first + 11, first + 12];
    nl = first + 13;
    let i64_locals = nl - 5;
    let next = nl;
    let l = Locals { reg, s, c, left, exit_pc, exit_unrun, t, next };
    let n = region.len() as u32;
    let mut g = Gen { c: Vec::new(), l, n, k: 0, extra: 0, flags_live };
    // locals: i64 × i64_locals, then one i32
    leb_u(&mut g.c, 2);
    leb_u(&mut g.c, i64_locals as u64);
    g.c.push(I64T);
    leb_u(&mut g.c, 1);
    g.c.push(I32);

    // registers in, budget, flags
    for r in 0..27 {
        if used[r] {
            g.get(P_CPU);
            g.mem(I64_LOAD, 3, OFF_REGS + 8 * r);
            g.set(reg[r]);
        }
    }
    g.get(P_BUDGET);
    g.set(left);
    g.flags_from_cpu();
    // the entry block: by its offset (none: leave at once)
    let blocks: HashMap<i64, u32> = region.iter().enumerate().map(|(i, rb)| (rb.off, i as u32)).collect();
    g.i32c(n as i64);
    g.set(next);
    for (i, rb) in region.iter().enumerate() {
        g.i32c(i as i64);
        g.get(next);
        g.get(P_ENTRY);
        g.i64c(rb.off);
        g.op(I64_EQ);
        g.op(SELECT);
        g.set(next);
    }

    // block $exit; loop $dispatch; block $default; block $b(n-1) .. block $b0
    g.c.extend_from_slice(&[BLOCK, 0x40, LOOP, 0x40, BLOCK, 0x40]);
    for _ in 0..n {
        g.c.extend_from_slice(&[BLOCK, 0x40]);
    }
    g.get(next);
    g.c.push(BR_TABLE);
    leb_u(&mut g.c, n as u64);
    for i in 0..n {
        leb_u(&mut g.c, i as u64);
    }
    leb_u(&mut g.c, n as u64);

    for (k, rb) in region.iter().enumerate() {
        g.c.push(END); // the code of block k follows the end of $bk
        g.k = k as u32;
        g.extra = 0;
        block(&mut g, rb, &blocks, keep);
    }
    g.c.push(END); // $default: an entry offset that is no block of the region
    g.get(P_VA);
    g.get(P_ENTRY);
    g.op(I64_ADD);
    g.set(exit_pc);
    g.i64c(0);
    g.set(exit_unrun);
    g.br(BR, 1);
    g.c.extend_from_slice(&[END, END]); // the loop, $exit

    // exit: flags and the PC; steps = budget − left − unrun
    g.s_to_cpu();
    g.get(P_CPU);
    g.get(exit_pc);
    g.mem(I64_STORE, 3, OFF_PC);
    g.get(P_BUDGET);
    g.get(left);
    g.op(I64_SUB);
    g.get(exit_unrun);
    g.op(I64_SUB);
    g.op(RETURN);
    g.c.push(END);
    g.c
}

/// The code of one block: it always ends by jumping or leaving.
#[allow(clippy::vec_box)]
fn block(g: &mut Gen, rb: &RegionBlock, blocks: &HashMap<i64, u32>, keep: &mut Vec<Box<Dec>>) {
    let count = rb.ops.iter().take_while(|d| d.op != Op::Generic).count() as i64;
    // leave before the block if the budget cannot cover it
    g.get(g.l.left);
    g.i64c(count.max(1));
    g.op(I64_LT_S);
    g.open(IF);
    g.leave_at(rb.off, 0);
    g.end();
    g.get(g.l.left);
    g.i64c(count);
    g.op(I64_SUB);
    g.set(g.l.left);

    let [t0, t1, t2, _, _, _, t6, _] = g.l.t;
    let mut off = rb.off;
    for (k, d) in rb.ops.iter().enumerate() {
        let unrun = count - k as i64;
        let next_off = off + d.len as i64;
        let (x, y, z) = (d.x as i64, d.y as i64, d.z as i64);
        match d.op {
            Op::AddR | Op::SubR | Op::AddK | Op::SubK => {
                g.reg(y);
                match d.op {
                    Op::AddR | Op::SubR => g.reg(z),
                    _ => g.i64c(d.imm),
                }
                g.op(if matches!(d.op, Op::AddR | Op::AddK) { I64_ADD } else { I64_SUB });
                g.tee(t0);
                g.set(g.l.c);
                g.wrap(t0);
                g.get(t0);
                g.set(g.l.s);
                g.set_reg(x, t0);
            }
            Op::CmpR | Op::CmpK => {
                g.reg(y);
                if d.op == Op::CmpR {
                    g.reg(z);
                } else {
                    g.i64c(d.imm);
                }
                g.op(I64_SUB);
                g.set(g.l.s);
            }
            Op::AluR | Op::AluK if matches!(d.f as i64, alu::MUL | alu::MULH) => {
                g.reg(y);
                g.set(t1);
                if d.op == Op::AluR {
                    g.reg(z);
                } else {
                    g.i64c(d.imm);
                }
                g.set(t2);
                // hi = nearest(x * y / 3^27) (f64), lo = x*y - hi*3^27, then
                // one correction each way
                g.get(t1);
                g.op(F64_CONVERT_I64_S);
                g.get(t2);
                g.op(F64_CONVERT_I64_S);
                g.op(F64_MUL);
                g.op(F64_CONST);
                g.c.extend_from_slice(&(W_MOD as f64).to_le_bytes());
                g.op(F64_DIV);
                g.op(F64_NEAREST);
                g.op(PREFIX_FC);
                leb_u(&mut g.c, I64_TRUNC_SAT_F64_S as u64);
                g.set(t0); // hi
                g.get(t1);
                g.get(t2);
                g.op(I64_MUL);
                g.get(t0);
                g.i64c(W_MOD);
                g.op(I64_MUL);
                g.op(I64_SUB);
                g.set(t1); // lo
                // over: lo -= W_MOD, hi += 1; under: lo += W_MOD, hi -= 1
                g.get(t1);
                g.i64c(W_MAX);
                g.op(I64_GT_S);
                g.op(I64_EXTEND_I32_U);
                g.get(t1);
                g.i64c(-W_MAX);
                g.op(I64_LT_S);
                g.op(I64_EXTEND_I32_U);
                g.op(I64_SUB);
                g.set(t2); // +1, 0, -1
                g.get(t0);
                g.get(t2);
                g.op(I64_ADD);
                g.set(t0);
                g.get(t1);
                g.get(t2);
                g.i64c(W_MOD);
                g.op(I64_MUL);
                g.op(I64_SUB);
                g.set(t1);
                if d.f as i64 == alu::MUL {
                    // C: the sign of the high word, as a raw sum
                    g.get(t0);
                    g.sign();
                    g.i64c(W_MAX + 1);
                    g.op(I64_MUL);
                    g.set(g.l.c);
                    g.get(t1);
                    g.set(g.l.s);
                    g.set_reg(x, t1);
                } else {
                    g.get(t0);
                    g.set(g.l.s);
                    g.set_reg(x, t0);
                }
            }
            Op::AluR | Op::AluK if matches!(d.f as i64, alu::DIV | alu::MOD) && (d.op == Op::AluR || d.imm != 0) => {
                if d.op == Op::AluR {
                    g.reg(z);
                } else {
                    g.i64c(d.imm);
                }
                g.set(t2);
                // a zero divisor goes to the interpreter, which faults
                g.get(t2);
                g.op(I64_EQZ);
                g.open(IF);
                g.exec_one(d, off, next_off, unrun, keep);
                g.leave_at(off, unrun);
                g.end();
                g.reg(y);
                g.get(t2);
                g.op(if d.f as i64 == alu::DIV { I64_DIV_S } else { I64_REM_S });
                g.tee(t0);
                g.set(g.l.s);
                g.set_reg(x, t0);
            }
            Op::AluR | Op::AluK if alu_by_helper(d.f as i64) => {
                g.i64c(d.f as i64);
                g.reg(y);
                if d.op == Op::AluR {
                    g.reg(z);
                } else {
                    g.i64c(d.imm);
                }
                g.call(T_ALU, jit_alu as *const () as usize);
                g.tee(t0);
                g.set(g.l.s);
                g.set_reg(x, t0);
            }
            Op::AluR | Op::AluK if matches!(d.f as i64, alu::NEG | alu::SGN | alu::ABS) => {
                match d.f as i64 {
                    alu::NEG => {
                        g.i64c(0);
                        g.reg(y);
                        g.op(I64_SUB);
                    }
                    alu::SGN => {
                        g.reg(y);
                        g.sign();
                    }
                    _ => {
                        // |a| = a < 0 ? -a : a
                        g.i64c(0);
                        g.reg(y);
                        g.op(I64_SUB);
                        g.reg(y);
                        g.reg(y);
                        g.i64c(0);
                        g.op(I64_LT_S);
                        g.op(SELECT);
                    }
                }
                g.tee(t0);
                g.set(g.l.s);
                g.set_reg(x, t0);
            }
            Op::Li => {
                g.i64c(d.imm);
                g.set(t0);
                g.set_reg(x, t0);
            }
            Op::Lea => {
                g.pc_any(off + d.imm, t0);
                g.set_reg(x, t0);
            }
            Op::Ldw | Op::Stw | Op::Ldt | Op::Stt => {
                let word = matches!(d.op, Op::Ldw | Op::Stw);
                let store = matches!(d.op, Op::Stw | Op::Stt);
                // the address unwrapped: a miss if it would wrap, and the
                // helper wraps it
                g.reg(y);
                g.i64c(d.imm);
                g.op(I64_ADD);
                g.set(t1); // va
                g.window(t1, store, !word, t2, t6);
                g.open(IF);
                // through the window
                if word && !store {
                    g.get(t2);
                    g.op(I32_WRAP_I64);
                    g.mem(I64_LOAD, 3, 0);
                    g.set(t0);
                    g.set_reg(x, t0);
                } else if word {
                    g.get(t2);
                    g.op(I32_WRAP_I64);
                    g.reg(x);
                    g.mem(I64_STORE, 3, 0);
                } else {
                    // the word, and its tryte at place t6 (0 high .. 2 low)
                    g.get(t2);
                    g.op(I32_WRAP_I64);
                    g.mem(I64_LOAD, 3, 0);
                    g.set(t0); // w
                    let (lo, rest, old) = (g.l.t[3], g.l.t[4], g.l.t[5]);
                    // t2' = low(w); rest = (w - low) / 3^9; t1' = low(rest); t0' = (rest - t1') / 3^9
                    g.low_tryte(t0);
                    g.set(lo);
                    g.get(t0);
                    g.get(lo);
                    g.op(I64_SUB);
                    g.i64c(T9_INV);
                    g.op(I64_MUL);
                    g.set(rest);
                    // old = place 2 ? low : place 1 ? low(rest) : (rest - low(rest)) / 3^9
                    g.low_tryte(rest); // t1'
                    g.set(old);
                    g.get(rest);
                    g.get(old);
                    g.op(I64_SUB);
                    g.i64c(T9_INV);
                    g.op(I64_MUL); // t0'
                    g.get(old); // t1'
                    g.get(t6);
                    g.i64c(0);
                    g.op(I64_EQ);
                    g.op(SELECT); // place 0 ? t0' : t1'
                    g.get(lo); // t2'
                    g.get(t6);
                    g.i64c(2);
                    g.op(I64_NE);
                    g.op(SELECT); // place 2 ? t2' : the above
                    g.set(old);
                    if !store {
                        g.set_reg(x, old);
                    } else {
                        // w + (new - old) * 3^(9 * (2 - place))
                        let v = g.l.t[3];
                        g.reg(x);
                        g.set(v);
                        g.get(t2);
                        g.op(I32_WRAP_I64);
                        g.get(t0);
                        g.low_tryte(v);
                        g.get(old);
                        g.op(I64_SUB);
                        g.i64c(T9 * T9);
                        g.i64c(T9);
                        g.i64c(1);
                        g.get(t6);
                        g.i64c(1);
                        g.op(I64_EQ);
                        g.op(SELECT);
                        g.get(t6);
                        g.i64c(0);
                        g.op(I64_EQ);
                        g.op(SELECT);
                        g.op(I64_MUL);
                        g.op(I64_ADD);
                        g.mem(I64_STORE, 3, 0);
                    }
                }
                g.else_();
                // the helper: TLB, a new window, or a fault
                g.get(P_CPU);
                g.get(P_MEM);
                g.get(t1);
                if store {
                    g.reg(x);
                    g.call(T_STORE, if word { jit_store_word as *const () as usize } else { jit_store_tryte as *const () as usize });
                } else {
                    g.get(P_CPU);
                    g.i32c(OFF_SCRATCH as i64);
                    g.op(I32_ADD);
                    g.call(T_LOAD, if word { jit_load_word as *const () as usize } else { jit_load_tryte as *const () as usize });
                }
                g.set(t0);
                g.status_fault(t0, off, unrun);
                if store {
                    g.status_dirty(t0, next_off, unrun);
                } else {
                    g.get(P_CPU);
                    g.mem(I64_LOAD, 3, OFF_SCRATCH);
                    g.set(t0);
                    g.set_reg(x, t0);
                }
                g.end();
            }
            Op::Jal => {
                g.pc_any(next_off, t0);
                g.set_reg(x, t0);
                g.goto(blocks, off + 3 * d.imm);
                return;
            }
            Op::Br | Op::Br3 => {
                if d.op == Op::Br {
                    // taken when the flag's value has its mask bit set
                    let mut any = false;
                    for kk in 0..3i64 {
                        if d.imm2 >> kk & 1 != 0 {
                            if d.f == 1 {
                                g.get(g.l.s);
                                g.i64c(0);
                                g.op([I64_LT_S, I64_EQ, I64_GT_S][kk as usize]);
                            } else {
                                g.carry();
                                g.i64c(kk - 1);
                                g.op(I64_EQ);
                            }
                            if any {
                                g.op(I32_OR);
                            }
                            any = true;
                        }
                    }
                    if any {
                        g.open(IF);
                        g.goto(blocks, off + 3 * d.imm);
                        g.end();
                    }
                } else {
                    g.get(g.l.s);
                    g.i64c(0);
                    g.op(I64_LT_S);
                    g.open(IF);
                    g.goto(blocks, off + 3 * d.imm);
                    g.end();
                    g.get(g.l.s);
                    g.i64c(0);
                    g.op(I64_GT_S);
                    g.open(IF);
                    g.goto(blocks, off + 3 * d.imm2 as i64);
                    g.end();
                }
                g.goto(blocks, next_off);
                return;
            }
            Op::Jalr => {
                g.reg(y);
                g.i64c(d.imm);
                g.op(I64_ADD);
                g.set(t1);
                g.wrap(t1); // the target, before the link may overwrite y
                g.pc_any(next_off, t0);
                g.set_reg(x, t0);
                g.leave_t(t1, 0);
                return;
            }
            Op::Generic => {
                g.leave_at(off, 0);
                return;
            }
            _ => g.exec_one(d, off, next_off, unrun, keep),
        }
        off = next_off;
    }
    g.goto(blocks, off); // fell off the end (length limit)
}

