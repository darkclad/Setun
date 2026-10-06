//! What both JIT back ends share: the region a request carries, the
//! compiled function's signature, and the helpers compiled code calls
//! (Cranelift's native code, and the WebAssembly modules of the browser
//! build).

use super::*;

/// (cpu, memory, the region root's address, budget, the entry block's
/// offset from the root) -> steps run.
pub(super) type NativeFn = unsafe extern "C" fn(*mut Cpu, *mut Memory, i64, i64, i64) -> i64;

/// One basic block of a region: its offset in trytes from the region's
/// start, and its instructions.
pub(super) struct RegionBlock {
    pub off: i64,
    pub ops: Vec<Dec>,
}

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

// ---- helpers called from compiled code ----------------------------------------

/// Status codes from helpers.
pub(super) const OK: i64 = 0;
pub(super) const OK_EXIT: i64 = 1; // done, but it wrote code or a page table: leave
pub(super) const FAULT: i64 = 2;

pub(super) unsafe extern "C" fn jit_load_word(cpu: *mut Cpu, mem: *mut Memory, va: i64, out: *mut i64) -> i64 {
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

pub(super) unsafe extern "C" fn jit_store_word(cpu: *mut Cpu, mem: *mut Memory, va: i64, v: i64) -> i64 {
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
pub(super) unsafe extern "C" fn jit_load_tryte(cpu: *mut Cpu, mem: *mut Memory, va: i64, out: *mut i64) -> i64 {
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
pub(super) unsafe extern "C" fn jit_store_tryte(cpu: *mut Cpu, mem: *mut Memory, va: i64, v: i64) -> i64 {
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
pub(super) fn alu_by_helper(f: i64) -> bool {
    matches!(f, alu::MIN | alu::MAX | alu::TMUL | alu::TSAT | alu::CONS | alu::SH | alu::TRIT | alu::TLZ | alu::BSH)
}

/// One of those (`x`, `y`: words).
pub(super) extern "C" fn jit_alu(f: i64, x: i64, y: i64) -> i64 {
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
pub(super) unsafe extern "C" fn jit_fcmp(cpu: *const Cpu, y: i64, z: i64, _: i64) -> i64 {
    let cpu = &*cpu;
    match cpu.freg(y).cmp(cpu.freg(z)) {
        Some(o) => o as i64,
        None => 2,
    }
}

/// The three words of fx as a real81 (FSTD stores them).
pub(super) unsafe extern "C" fn jit_real_words(cpu: *const Cpu, x: i64, out: *mut i64, _: i64) -> i64 {
    let ws = (*cpu).freg(x).to_words();
    std::ptr::copy_nonoverlapping(ws.as_ptr(), out, 3);
    OK
}

/// After the interpreter's access at `va` went through: its page into the
/// window table (`write`: the write half).
pub(super) unsafe extern "C" fn jit_fill_window(cpu: *mut Cpu, va: i64, write: i64, _: i64) -> i64 {
    (*cpu).jw_fill(va, write != 0);
    OK
}

/// Any non-control instruction, by the interpreter (its registers and the
/// flags are in `cpu` around the call).
pub(super) unsafe extern "C" fn jit_exec_one(cpu: *mut Cpu, mem: *mut Memory, d: *const Dec, pc: i64) -> i64 {
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
