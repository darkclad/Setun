//! Instruction-level tests against docs/SETUN26_ISA.md v0.4.

use setun_core::cpu::{Access, Cpu, Step};
use setun_core::isa::{self, alu, cond, cr, fld, mem, mode, op, sys, vector};
use setun_core::{Io, Memory};
use ternary::Word;

// ---- harness ---------------------------------------------------------------

/// Program builder: words at consecutive word addresses from `origin`.
struct Prog {
    origin: i64,
    words: Vec<Word>,
}

impl Prog {
    fn at(origin: i64) -> Self {
        assert_eq!(origin.rem_euclid(3), 0);
        Prog { origin, words: Vec::new() }
    }
    /// Word index of the next instruction (branch offsets count words).
    fn here(&self) -> i64 {
        self.words.len() as i64
    }
    fn i(&mut self, w: Word) -> &mut Self {
        self.words.push(w);
        self
    }
    fn i2(&mut self, w: [Word; 2]) -> &mut Self {
        self.words.extend(w);
        self
    }
    fn load(&self, m: &mut Memory) {
        m.load_words(self.origin, &self.words);
    }
}

fn word(v: i64) -> Word {
    Word::new(v).unwrap()
}

// Short encoders.
fn r(b: i64, rd: i64, ra: i64, rb: i64) -> Word {
    isa::rrr(op::ALU, b, rd, ra, rb)
}
fn q(b: i64, rd: i64, ra: i64, q3: i64) -> Word {
    isa::rrr(op::ALUQ, b, rd, ra, q3)
}
fn liq(rd: i64, v: i64) -> Word {
    isa::s6(op::LIQ, 0, rd, v)
}
fn li(rd: i64, v: i64) -> [Word; 2] {
    isa::wide(op::LI, 0, rd, 0, v)
}
fn brq(c: i64, off: i64) -> Word {
    isa::s6(op::BRQ, 0, c, off)
}
fn sysop(b: i64, x: i64, y: i64) -> Word {
    isa::rrr(op::SYS, b, x, y, 0)
}
fn hlt() -> Word {
    sysop(sys::HLT, 0, 0)
}

#[derive(Default)]
struct TestIo {
    out: Vec<(i64, i64)>,
    pending: Option<i64>,
}

impl Io for TestIo {
    fn port_in(&mut self, port: i64) -> Word {
        match port {
            164 => word(1), // serial TX_READY (block 162 + 2)
            _ => word(port * 10),
        }
    }
    fn port_out(&mut self, port: i64, value: Word) {
        self.out.push((port, value.value()));
    }
    fn pending_vector(&mut self) -> Option<i64> {
        self.pending.take()
    }
}

struct M {
    cpu: Cpu,
    mem: Memory,
    io: TestIo,
}

impl M {
    fn new(frames: i64) -> Self {
        M { cpu: Cpu::new(), mem: Memory::new(frames), io: TestIo::default() }
    }
    /// Load `p`, start at its origin and run until HLT (or `limit` steps).
    fn run(&mut self, p: &Prog) -> usize {
        p.load(&mut self.mem);
        self.cpu.pc = word(p.origin);
        self.resume()
    }
    fn resume(&mut self) -> usize {
        self.cpu.halted = false;
        for n in 0..10_000 {
            match self.cpu.step(&mut self.mem, &mut self.io) {
                Step::Ran => {}
                Step::Halted => return n,
                Step::TripleFault => panic!("triple fault"),
            }
        }
        panic!("did not halt");
    }
    fn r(&self, n: i64) -> i64 {
        self.cpu.reg(n).value()
    }
    fn cr(&self, n: i64) -> i64 {
        self.cpu.cr(n).value()
    }
    /// Install a vector table at `itb` where every vector points at a
    /// handler that just halts. Returns where PC rests after that `HLT`.
    fn halting_vectors(&mut self, itb: i64) -> i64 {
        let handler = itb + 3 * 200;
        let mut p = Prog::at(itb - 3 * 121);
        for _ in -121..=121 {
            p.i(word(handler));
        }
        p.load(&mut self.mem);
        Prog::at(handler).i(hlt()).load(&mut self.mem);
        self.cpu.set_cr(cr::ITB, word(itb));
        handler + 3
    }
}

// ---- tests -----------------------------------------------------------------

#[test]
fn reset_state() {
    let c = Cpu::new();
    assert_eq!(c.pc.value(), -59_049);
    assert_eq!(c.ring(), 1);
    assert_eq!(c.mode(mode::PG), 0);
    assert_eq!(c.mode(mode::IE), 0);
}

#[test]
fn add_carry_and_sign() {
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    p.i2(li(1, Word::MAX.value()))
        .i(liq(2, 1))
        .i(r(alu::ADD, 3, 1, 2)) // MAX + 1 wraps to MIN, C = +1
        .i(hlt());
    m.run(&p);
    assert_eq!(m.r(3), Word::MIN.value());
    assert_eq!(m.cpu.c(), 1);
    assert_eq!(m.cpu.s(), -1);

    let mut p = Prog::at(0);
    p.i(liq(1, 5))
        .i(liq(2, 7))
        .i(r(alu::SUB, 3, 1, 2))
        .i(liq(4, 9))
        .i(r(alu::ADC, 5, 4, 0)) // 9 + 0 + C(=0)
        .i(hlt());
    m.run(&p);
    assert_eq!(m.r(3), -2);
    assert_eq!(m.r(5), 9);
    assert_eq!(m.cpu.c(), 0);
}

#[test]
fn r0_is_hardwired_zero() {
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    p.i(liq(0, 42)).i(r(alu::ADD, 1, 0, 0)).i(hlt());
    m.run(&p);
    assert_eq!(m.r(0), 0);
    assert_eq!(m.r(1), 0);
}

#[test]
fn multiply_and_divide() {
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    p.i2(li(1, 3_000_000_000_000))
        .i(liq(2, 3))
        .i(r(alu::MUL, 3, 1, 2))
        .i(r(alu::MULH, 4, 1, 2))
        .i(liq(5, -7))
        .i(liq(6, 2))
        .i(r(alu::DIV, 7, 5, 6)) // -3 (truncated)
        .i(r(alu::MOD, 8, 5, 6)) // -1
        .i(liq(9, 8))
        .i(liq(10, 3))
        .i(r(alu::DIVR, 11, 9, 10)) // 8/3 = 2.67 -> 3
        .i(r(alu::MODR, 12, 9, 10)) // 8 - 9 = -1
        .i(hlt());
    m.run(&p);
    let prod = 9_000_000_000_000i128;
    assert_eq!(m.r(4) as i128 * 3i128.pow(27) + m.r(3) as i128, prod);
    assert_eq!(m.cpu.c(), 1, "MUL overflowed and set C; MULH and DIV leave it");
    assert_eq!((m.r(7), m.r(8)), (-3, -1));
    assert_eq!((m.r(11), m.r(12)), (3, -1));
}

#[test]
fn divide_by_zero_traps() {
    let mut m = M::new(1);
    let h = m.halting_vectors(3 * 3000);
    let mut p = Prog::at(0);
    p.i(liq(1, 5)).i(r(alu::DIV, 2, 1, 0)).i(hlt());
    m.run(&p);
    assert_eq!(m.cpu.pc.value(), h);
    assert_eq!(m.cr(cr::EPC), 3, "EPC = the faulting DIV");
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::DIVIDE);
}

#[test]
fn ternary_logic_and_shifts() {
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    p.i(liq(1, 100))
        .i(q(alu::SH, 2, 1, 2)) // 100 * 9
        .i(q(alu::SH, 3, 1, -2)) // round(100 / 9) = 11
        .i2(isa::wide(op::ALUI, alu::TRIT, 4, 1, 27)) // lowest trit of 100 (++-0+ → +1)
        .i(r(alu::NEG, 5, 1, 0))
        .i(r(alu::TMUL, 6, 1, 5)) // tritwise a*(-a) = -(a*a): each nonzero trit -> -1
        .i(r(alu::TLZ, 7, 1, 0))
        .i(q(alu::CMP, 0, 1, 13)) // S = sign(100 - 13)
        .i(hlt());
    m.run(&p);
    assert_eq!(m.r(2), 900);
    assert_eq!(m.r(3), 11);
    assert_eq!(m.r(4), 1);
    assert_eq!(m.r(5), -100);
    let expect = Word::new(100).unwrap().tritwise_mul(Word::new(-100).unwrap());
    assert_eq!(m.r(6), expect.value());
    assert_eq!(m.r(7), 22); // 100 = 3^4·1.23 → 5 significant trits
    assert_eq!(m.cpu.s(), 1);
}

#[test]
fn loads_stores_and_alignment() {
    let mut m = M::new(1);
    let h = m.halting_vectors(3 * 3000);
    let mut p = Prog::at(0);
    p.i(liq(1, 300)) // base
        .i2(li(2, -123_456_789))
        .i(isa::rrr(op::MEM, mem::STW, 2, 1, 1)) // [300 + 3] = r2
        .i(isa::rrr(op::MEM, mem::LDW, 3, 1, 1))
        .i2(isa::wide(op::MEMD, mem::LDT, 4, 1, 4)) // low tryte of that word (at 304)
        .i(liq(5, -77))
        .i(isa::rrr(op::MEM, mem::STT, 5, 1, -1)) // tryte [299] = -77
        .i(isa::rrr(op::MEM, mem::LDT, 6, 1, -1))
        .i(liq(7, 11))
        .i(isa::rrr(op::MEM, mem::SWAP, 7, 1, 1)) // r7 <-> [303]
        .i(isa::rrr(op::MEM, mem::LDW, 8, 1, 1))
        .i(hlt());
    m.run(&p);
    assert_eq!(m.r(3), -123_456_789);
    let low = setun_core::memory::word_to_trytes(word(-123_456_789))[2];
    assert_eq!(m.r(4), low.value());
    assert_eq!(m.r(6), -77);
    assert_eq!((m.r(7), m.r(8)), (-123_456_789, 11));

    // Misaligned word access traps with vector −5.
    let mut p = Prog::at(0);
    p.i(liq(1, 301)).i(isa::rrr(op::MEM, mem::LDW, 2, 1, 0)).i(hlt());
    m.run(&p);
    assert_eq!(m.cpu.pc.value(), h);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::MISALIGNED);
    assert_eq!(m.cr(cr::FADDR), 301);
}

#[test]
fn branches_loop_and_three_way() {
    // Sum 1..=10 with a BRQ loop.
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    p.i(liq(1, 10)).i(liq(2, 0));
    let top = p.here();
    p.i(r(alu::ADD, 2, 2, 1)).i(q(alu::SUB, 1, 1, 1));
    let off = top - p.here();
    p.i(brq(cond::GT, off)).i(hlt());
    m.run(&p);
    assert_eq!(m.r(2), 55);

    // BR3 dispatch on CMP: r3 = -1 / 0 / +1 path taken.
    for (a, b, expect) in [(3, 9, 100), (9, 9, 200), (9, 3, 300)] {
        let mut p = Prog::at(0);
        p.i(liq(1, a)).i(liq(2, b)).i(r(alu::CMP, 0, 1, 2));
        let at = p.here();
        // layout after BR3 (2 words): [eq: LIQ 200; HLT] [lt: LIQ 100; HLT] [gt: LIQ 300; HLT]
        p.i2(isa::br3(at + 4 - at, at + 6 - at))
            .i(liq(3, 200))
            .i(hlt())
            .i(liq(3, 100))
            .i(hlt())
            .i(liq(3, 300))
            .i(hlt());
        m.run(&p);
        assert_eq!(m.r(3), expect, "{a} vs {b}");
    }
}

#[test]
fn overflow_condition_uses_carry() {
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    p.i2(li(1, Word::MAX.value()))
        .i(q(alu::ADD, 1, 1, 1))
        .i(brq(cond::OV, 3))
        .i(liq(2, 1))
        .i(hlt())
        .i(liq(2, 2))
        .i(hlt());
    m.run(&p);
    assert_eq!(m.r(2), 2);
}

#[test]
fn call_and_return() {
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    // main: r1 = 20; CALL double; HLT     double: r1 += r1; RET
    p.i(liq(1, 20));
    let call = p.here();
    p.i2(isa::wide(op::JAL, 0, 12, 0, 3)); // to `double`, 3 words ahead
    p.i(hlt());
    assert_eq!(p.here(), call + 3);
    p.i(r(alu::ADD, 1, 1, 1)).i(isa::rrr(op::JALR, 0, 0, 12, 0));
    m.run(&p);
    assert_eq!(m.r(1), 40);
    assert_eq!(m.r(12), 3 * (call + 2), "link = address after the 2-word JAL");
}

#[test]
fn lea_and_fld() {
    let mut m = M::new(1);
    let mut p = Prog::at(30);
    p.i2(isa::wide(op::LEA, 0, 1, 0, 1000))
        .i2(li(2, Word::from_b27("000000ABC").unwrap().value()))
        .i2(isa::fld(fld::EXT, 3, 2, 19, 9)) // low tryte = ABC
        .i(liq(4, 0))
        .i2(isa::fld(fld::INS, 4, 2, 1, 9)) // put the low 9 trits of r2 at the top
        .i(hlt());
    m.run(&p);
    assert_eq!(m.r(1), 1030);
    assert_eq!(m.r(3), Word::from_b27("ABC").unwrap().value());
    assert_eq!(m.cpu.reg(4).to_b27(), "ABC000000");
}

#[test]
fn hello_example_from_spec() {
    // ISA §11, with putc inlined as a subroutine.
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    p.i(liq(1, 'H' as i64)).i2(isa::wide(op::JAL, 0, 12, 0, 6));
    p.i(liq(1, 'i' as i64)).i2(isa::wide(op::JAL, 0, 12, 0, 3));
    p.i(hlt());
    // putc at word 7
    assert_eq!(p.here(), 7);
    p.i(liq(3, 162));
    let wait = p.here();
    p.i(isa::rrr(op::IO, isa::io::IN, 2, 3, 2))
        .i(q(alu::CMP, 0, 2, 0));
    let off = wait - p.here();
    p.i(brq(cond::LE, off))
        .i(isa::rrr(op::IO, isa::io::OUT, 1, 3, 0))
        .i(isa::rrr(op::JALR, 0, 0, 12, 0));
    m.run(&p);
    assert_eq!(m.io.out, vec![(162, 'H' as i64), (162, 'i' as i64)]);
}

#[test]
fn illegal_and_privileged() {
    let mut m = M::new(1);
    let h = m.halting_vectors(3 * 3000);
    // An all-zero word is ILL.
    let p = Prog::at(0);
    m.run(&p);
    assert_eq!(m.cpu.pc.value(), h);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::ILLEGAL);

    // A reserved A[1] = − major is ILL too.
    let mut p = Prog::at(0);
    p.i(isa::rrr(-6561, 0, 0, 0, 0));
    m.run(&p);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::ILLEGAL);
}

/// Kernel starts a ring −1 program; the program makes a system call.
#[test]
fn syscall_switches_ring_and_stack() {
    let mut m = M::new(2);
    let itb = 3 * 3000;
    m.halting_vectors(itb);
    // Kernel: set up EMODE (ring −1, IE 0), EPC = user entry, r13 = kernel sp
    // then IRET.
    let user = 3 * 5000;
    let mut k = Prog::at(0);
    k.i2(li(1, user))
        .i(sysop(sys::MTC, cr::EPC, 1))
        .i2(li(1, Word::ZERO.with_field(27..=27, -1).value()))
        .i(sysop(sys::MTC, cr::EMODE, 1))
        .i2(li(13, 30_000)) // kernel sp
        .i2(li(1, 20_000)) // user sp, parked in KSP until IRET swaps
        .i(sysop(sys::MTC, cr::KSP, 1))
        .i(sysop(sys::IRET, 0, 0));
    k.load(&mut m.mem);
    // User: SYS 42 (from ring −1).
    let mut u = Prog::at(user);
    u.i(liq(5, 7)).i(isa::s9(op::SYS, sys::SYS, 42)).i(hlt());
    u.load(&mut m.mem);
    m.cpu.pc = word(0);
    m.resume();

    assert_eq!(m.cpu.ring(), 1, "trap entered ring +1");
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::SYSCALL);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(1..=18), 42);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(21..=21), -1, "caller ring");
    assert_eq!(m.cr(cr::EPC), user + 6, "EPC = after SYS");
    assert_eq!(m.cpu.cr(cr::EMODE).trit(27) as i64, -1);
    assert_eq!(m.r(13), 30_000, "kernel stack");
    assert_eq!(m.cr(cr::KSP), 20_000, "user stack parked in KSP");
    assert_eq!(m.r(5), 7);
}

#[test]
fn user_mode_cannot_touch_control_registers_or_ports() {
    let mut m = M::new(1);
    m.halting_vectors(3 * 3000);
    m.cpu.set_mode(mode::RING, -1);
    let mut p = Prog::at(0);
    p.i(sysop(sys::MTC, cr::PTB, 1));
    m.run(&p);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::PRIVILEGE);

    m.cpu.set_mode(mode::RING, -1);
    let mut p = Prog::at(0);
    p.i(isa::rrr(op::IO, isa::io::IN, 1, 0, 0));
    m.run(&p);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::PRIVILEGE);
}

#[test]
fn interrupts_and_single_step() {
    let mut m = M::new(1);
    let h = m.halting_vectors(3 * 3000) - 3; // handler entry (we single-step)
    let mut p = Prog::at(0);
    p.i(liq(1, 1)).i(liq(2, 2)).i(hlt());
    p.load(&mut m.mem);
    m.cpu.pc = word(0);
    // IE = 0: a pending interrupt is ignored.
    m.io.pending = Some(5);
    m.cpu.step(&mut m.mem, &mut m.io);
    assert_eq!(m.cpu.pc.value(), 3);
    // IE = +1: taken before the next instruction, EPC = that instruction.
    m.cpu.set_mode(mode::IE, 1);
    m.io.pending = Some(5);
    m.cpu.step(&mut m.mem, &mut m.io);
    assert_eq!(m.cpu.pc.value(), h);
    assert_eq!(m.cr(cr::EPC), 3);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), 5);
    assert_eq!(m.cpu.mode(mode::IE), 0);

    // TF: trap after one instruction with EPC = the next one.
    m.cpu.pc = word(0);
    m.cpu.set_mode(mode::TF, 1);
    m.cpu.step(&mut m.mem, &mut m.io);
    assert_eq!(m.cpu.pc.value(), h);
    assert_eq!(m.cr(cr::EPC), 3);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::STEP);
}

#[test]
fn missing_vector_table_is_a_triple_fault() {
    let mut m = M::new(1);
    m.cpu.set_cr(cr::ITB, word(-9 * 1_000_000)); // frame −457: nothing there
    Prog::at(0).load(&mut m.mem);
    m.cpu.pc = word(0);
    assert_eq!(m.cpu.step(&mut m.mem, &mut m.io), Step::TripleFault);
    assert_eq!(m.cpu.pc.value(), -59_049, "machine was reset");
}

#[test]
fn two_word_instruction_across_frames() {
    let mut m = M::new(2);
    // Last word of frame 0 is centred at 9840; the immediate sits in frame 1.
    let mut p = Prog::at(9840);
    p.i2(li(1, 777)).i(hlt());
    m.run(&p);
    assert_eq!(m.r(1), 777);
}

// ---- MMU -------------------------------------------------------------------

const L1: i64 = 59_049; // 3^10, frames 2..=4
const L2_FRAME: i64 = 6; // table centred at 6·19683 = 118098 (3·3^10), frames 5..=7

fn pte(frame: i64, v: i64, x: i64, ring: i64) -> Word {
    Word::ZERO
        .with_field(1..=18, frame)
        .with_field(19..=19, v)
        .with_field(20..=20, x)
        .with_field(21..=21, ring)
}

/// VA page (0, l2) → frame, with permissions.
fn map(m: &mut Memory, l2: i64, entry: Word) {
    m.write_word(L2_FRAME * 19_683 + 3 * l2, entry).unwrap();
}

fn paged_machine() -> M {
    let mut m = M::new(27);
    m.mem.write_word(L1, pte(L2_FRAME, 1, 1, -1)).unwrap();
    map(&mut m.mem, 0, pte(10, 1, 1, -1)); // user code/data, RWX
    map(&mut m.mem, 1, pte(11, -1, 0, -1)); // user read-only, no exec
    map(&mut m.mem, 3, pte(12, 1, 1, 1)); // kernel only: vectors + handler
    m.cpu.set_cr(cr::PTB, word(L1));
    m.cpu.set_mode(mode::PG, 1);
    m
}

#[test]
fn mmu_translates_and_sets_accessed_dirty() {
    let mut m = paged_machine();
    m.cpu.set_mode(mode::RING, -1);
    assert_eq!(m.cpu.translate(&mut m.mem, 5, Access::Read), Ok(10 * 19_683 + 5));
    assert_eq!(m.cpu.translate(&mut m.mem, 19_683 - 7, Access::Read), Ok(11 * 19_683 - 7));
    let ad = |m: &M, l2: i64| m.mem.read_word(L2_FRAME * 19_683 + 3 * l2).unwrap().trit(22) as i64;
    assert_eq!(ad(&m, 0), -1, "read marks accessed");
    m.cpu.translate(&mut m.mem, 5, Access::Write).unwrap();
    assert_eq!(ad(&m, 0), 1, "write marks dirty");
    m.cpu.translate(&mut m.mem, 5, Access::Read).unwrap();
    assert_eq!(ad(&m, 0), 1, "dirty stays dirty");

    let vec_of = |r: Result<i64, setun_core::cpu::Fault>| r.unwrap_err().vector;
    assert_eq!(vec_of(m.cpu.translate(&mut m.mem, 19_683, Access::Write)), vector::PROTECTION);
    assert_eq!(vec_of(m.cpu.translate(&mut m.mem, 19_683, Access::Fetch)), vector::PROTECTION);
    assert_eq!(vec_of(m.cpu.translate(&mut m.mem, 2 * 19_683, Access::Read)), vector::PAGE_FAULT);
    assert_eq!(vec_of(m.cpu.translate(&mut m.mem, 3 * 19_683, Access::Read)), vector::PROTECTION, "kernel page");
    m.cpu.set_mode(mode::RING, 1);
    assert!(m.cpu.translate(&mut m.mem, 3 * 19_683, Access::Read).is_ok());
    assert_eq!(vec_of(m.cpu.translate(&mut m.mem, 19_683 * 19_683, Access::Read)), vector::PAGE_FAULT, "L1 entry absent");
}

#[test]
fn user_program_under_paging_faults_into_kernel() {
    let mut m = paged_machine();
    let itb = 3 * 19_683; // VA page l2 = 3, kernel-only
    // Vector table + halting handler live in kernel page (physical frame 12).
    let handler = itb + 3 * 200;
    let mut vt = Prog::at(12 * 19_683 - 3 * 121);
    for _ in -121..=121 {
        vt.i(word(handler));
    }
    vt.load(&mut m.mem);
    Prog::at(12 * 19_683 + 3 * 200).i(hlt()).load(&mut m.mem);
    m.cpu.set_cr(cr::ITB, word(itb));

    // User code in VA page 0 (frame 10): store into the read-only page.
    let mut u = Prog::at(10 * 19_683); // physical; VA 0
    u.i2(li(1, 19_683)).i(isa::rrr(op::MEM, mem::STW, 2, 1, 0)).i(hlt());
    u.load(&mut m.mem);
    m.cpu.set_mode(mode::RING, -1);
    m.cpu.pc = word(0);
    m.resume();
    assert_eq!(m.cpu.pc.value(), handler + 3, "halted in the handler");
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::PROTECTION);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(22..=22), 1, "write access");
    assert_eq!(m.cr(cr::FADDR), 19_683);
    assert_eq!(m.cr(cr::EPC), 6);

    // A 2-word instruction whose second word falls on the no-exec page.
    m.cpu.set_cr(cr::MODE, m.cpu.cr(cr::EMODE)); // back to ring −1, paging on
    Prog::at(10 * 19_683 + 9840).i2(li(1, 5)).load(&mut m.mem);
    m.cpu.pc = word(9840);
    m.resume();
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::PROTECTION);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(22..=22), 0, "fetch access");
    assert_eq!(m.cr(cr::FADDR), 9843, "FADDR = the second word");
    assert_eq!(m.cr(cr::EPC), 9840);
}

// ---- binary operations (ISA §3.3.1) ----------------------------------------

const W_MAX: i64 = 3_812_798_742_493;
const W_MOD: i64 = 7_625_597_484_987;

/// One binary ALU operation on two values, run on the machine.
fn bin(f: i64, a: i64, b: i64) -> i64 {
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    p.i2(li(1, a)).i2(li(2, b)).i(r(f, 3, 1, 2)).i(hlt());
    m.run(&p);
    m.r(3)
}

fn field(f: i64, a: i64, pos: i64, len: i64) -> i64 {
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    p.i2(li(1, a)).i2(isa::fld(f, 3, 1, pos, len)).i(hlt());
    m.run(&p);
    m.r(3)
}

#[test]
fn binary_logic_on_32_bit_values() {
    let vals: [u32; 6] = [0xdead_beef, 0x0f0f_1234, 0, 0xffff_ffff, 0x8000_0001, 0x1234_5678];
    for &a in &vals {
        for &b in &vals {
            let (x, y) = (a as i64, b as i64);
            assert_eq!(bin(alu::BAND, x, y), (a & b) as i64, "{a:#x} & {b:#x}");
            assert_eq!(bin(alu::BOR, x, y), (a | b) as i64, "{a:#x} | {b:#x}");
            assert_eq!(bin(alu::BXOR, x, y), (a ^ b) as i64, "{a:#x} ^ {b:#x}");
            assert_eq!(bin(alu::BANDN, x, y), (a & !b) as i64, "{a:#x} & !{b:#x}");
        }
    }
}

#[test]
fn binary_logic_on_negative_and_huge_values() {
    // two's complement of unlimited width: −1 is all ones
    assert_eq!(bin(alu::BAND, -1, 12345), 12345);
    assert_eq!(bin(alu::BXOR, -1, 12345), -12346);
    assert_eq!(bin(alu::BOR, -5, 3), -5);
    assert_eq!(bin(alu::BAND, -8, -3), -8);
    assert_eq!(bin(alu::BANDN, 0xff, -16), 0xf);
    // results past the word range wrap modulo 3^27
    let low = (1i64 << 42) - 1 - W_MAX; // W_MAX's missing bits below 2^42
    assert_eq!(W_MAX | low, (1i64 << 42) - 1);
    assert_eq!(bin(alu::BOR, W_MAX, low), (1i64 << 42) - 1 - W_MOD);
    let wrap = |v: i64| if v > W_MAX { v - W_MOD } else if v < -W_MAX { v + W_MOD } else { v };
    let edges = [W_MAX, -W_MAX, low, -low, 1 << 41, -(1 << 41), W_MAX - 1, -W_MAX + 1, 0x2aa_aaaa_aaaa, -0x155_5555_5555];
    let mut wrapped = 0;
    for &a in &edges {
        for &b in &edges {
            for (f, v) in [(alu::BAND, a & b), (alu::BOR, a | b), (alu::BXOR, a ^ b), (alu::BANDN, a & !b)] {
                wrapped += (wrap(v) != v) as i32;
                assert_eq!(bin(f, a, b), wrap(v), "f {f} on {a} {b}");
            }
        }
    }
    assert!(wrapped > 10, "the edges wrap ({wrapped})");
}

#[test]
fn binary_shifts() {
    assert_eq!(bin(alu::BSH, 5, 3), 40);
    assert_eq!(bin(alu::BSH, -40, -3), -5);
    assert_eq!(bin(alu::BSH, -1, -1), -1); // floor, as an arithmetic shift
    assert_eq!(bin(alu::BSH, 7, -1), 3);
    assert_eq!(bin(alu::BSH, -7, -1), -4);
    assert_eq!(bin(alu::BSH, 1 << 41, 1), (1i64 << 42) - W_MOD); // wraps
    assert_eq!(bin(alu::BSH, 12345, 0), 12345);
    assert_eq!(bin(alu::BSH, W_MAX, -100), 0); // limited to −81
    assert_eq!(bin(alu::BSH, -W_MAX, -100), -1);
    // 3·2^81 modulo 3^27, from the definition
    let mut v: i128 = 3;
    for _ in 0..81 {
        v = (v * 2).rem_euclid(W_MOD as i128);
    }
    let want = if v > W_MAX as i128 { v - W_MOD as i128 } else { v } as i64;
    assert_eq!(bin(alu::BSH, 3, 9841), want);
}

#[test]
fn bit_fields() {
    assert_eq!(field(fld::BEXT, 0xdead_beef, 8, 8), 0xbe);
    assert_eq!(field(fld::BEXT, 0xdead_beef, 0, 32), 0xdead_beef);
    assert_eq!(field(fld::BEXT, 0x1_dead_beef, 0, 32), 0xdead_beef); // mod 2^32
    assert_eq!(field(fld::BEXT, -1, 70, 41), (1i64 << 41) - 1);
    assert_eq!(field(fld::BEXT, -2, 0, 4), 14);
    assert_eq!(field(fld::BEXT, W_MAX, 80, 1), 0);
    assert_eq!(field(fld::BROT, 0x8000_0001, 1, 32), 3);
    assert_eq!(field(fld::BROT, 0x1234_5678, 8, 32), 0x3456_7812);
    assert_eq!(field(fld::BROT, 0x1234_5678, 0, 32), 0x1234_5678);
    assert_eq!(field(fld::BROT, -1, 5, 41), (1i64 << 41) - 1);
    assert_eq!(field(fld::BROT, 0b1011, 1, 4), 0b0111);
}

/// The ChaCha20 quarter round in binary instructions: RFC 8439 §2.1.1.
#[test]
fn chacha_quarter_round() {
    let mut m = M::new(1);
    let mut p = Prog::at(0);
    p.i2(li(1, 0x1111_1111)).i2(li(2, 0x0102_0304)).i2(li(3, 0x9b8d_6f43)).i2(li(4, 0x0123_4567));
    // a += b; d ^= a; d <<<= 16 ... (each sum cut to 32 bits)
    for (a, b, d, n) in [(1, 2, 4, 16), (3, 4, 2, 12), (1, 2, 4, 8), (3, 4, 2, 7)] {
        p.i(r(alu::ADD, a, a, b))
            .i2(isa::fld(fld::BEXT, a, a, 0, 32))
            .i(r(alu::BXOR, d, d, a))
            .i2(isa::fld(fld::BROT, d, d, n, 32));
    }
    p.i(hlt());
    m.run(&p);
    assert_eq!([m.r(1), m.r(2), m.r(3), m.r(4)], [0xea2a_92f4, 0xcb1c_f8ce, 0x4581_472e, 0x5881_c4bb]);
}
