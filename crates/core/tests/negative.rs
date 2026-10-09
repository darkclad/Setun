//! Negative tests: illegal encodings, privilege and permission violations,
//! bad addresses, faults that must have no side effects, and devices fed
//! nonsense.

use setun_core::cpu::Access;
use setun_core::devices::{block, BlockStore, FileDisk, MemDisk};
use setun_core::isa::{self, alu, cond, cr, fld, mem, mode, op, sys, vector};
use setun_core::memory::{frame_start, BusError};
use setun_core::{Io, Machine, Memory, Step, PAGE};
use ternary::{Tryte, Word};

// ---- harness ---------------------------------------------------------------

const ITB: i64 = 3 * 3000;
const HANDLER: i64 = 3 * 3200;
const NO_FAULT: i64 = 999;

fn w(v: i64) -> Word {
    Word::new(v).unwrap()
}

/// A 2-frame machine whose every vector points at a `HLT` handler.
fn machine() -> Machine {
    let mut m = Machine::new(2, 0);
    let table: Vec<Word> = (-121..=121).map(|_| w(HANDLER)).collect();
    m.mem.load_words(ITB - 3 * 121, &table);
    m.mem.load_words(HANDLER, &[isa::rrr(op::SYS, sys::HLT, 0, 0, 0)]);
    m.cpu.set_cr(cr::ITB, w(ITB));
    m
}

/// Run `prog` from address 0 (a final HLT is appended). Returns the vector
/// of the fault taken, or `NO_FAULT` if the program reached its own HLT.
fn run(m: &mut Machine, prog: &[Word]) -> i64 {
    let mut p = prog.to_vec();
    p.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    m.mem.load_words(0, &p);
    m.cpu.pc = Word::ZERO;
    m.cpu.halted = false;
    for _ in 0..1000 {
        match m.step() {
            Step::Ran => {}
            Step::Halted => break,
            Step::TripleFault => return -1000,
        }
    }
    if m.cpu.pc.value() == HANDLER + 3 {
        m.cpu.cr(cr::FCAUSE).field(23..=27)
    } else {
        NO_FAULT
    }
}

fn fault(prog: &[Word]) -> i64 {
    run(&mut machine(), prog)
}

fn in_ring(ring: i64, prog: &[Word]) -> i64 {
    let mut m = machine();
    m.cpu.set_mode(mode::RING, ring);
    run(&mut m, prog)
}

fn rrr(a: i64, b: i64, x: i64, y: i64, z: i64) -> Word {
    isa::rrr(a, b, x, y, z)
}
fn liq(rd: i64, v: i64) -> Word {
    isa::s6(op::LIQ, 0, rd, v)
}
fn li(rd: i64, v: i64) -> [Word; 2] {
    isa::wide(op::LI, 0, rd, 0, v)
}
fn cat<const N: usize>(parts: [&[Word]; N]) -> Vec<Word> {
    parts.concat()
}

// ---- illegal encodings -----------------------------------------------------

#[test]
fn reserved_alu_functions_are_illegal() {
    for b in [0, -1, -22, -23, 32, 100, 9841] {
        assert_eq!(fault(&[rrr(op::ALU, b, 1, 2, 3)]), vector::ILLEGAL, "ALU B={b}");
        assert_eq!(fault(&[rrr(op::ALUQ, b, 1, 2, 3)]), vector::ILLEGAL, "ALUQ B={b}");
        assert_eq!(fault(&isa::wide(op::ALUI, b, 1, 2, 3)), vector::ILLEGAL, "ALUI B={b}");
    }
}

#[test]
fn reserved_memory_io_fld_sys_functions_are_illegal() {
    for b in [0, 6, -1] {
        assert_eq!(fault(&[rrr(op::MEM, b, 1, 0, 0)]), vector::ILLEGAL, "MEM B={b}");
        assert_eq!(fault(&isa::wide(op::MEMD, b, 1, 0, 0)), vector::ILLEGAL, "MEMD B={b}");
    }
    for b in [0, 3, -1] {
        assert_eq!(fault(&[rrr(op::IO, b, 1, 0, 0)]), vector::ILLEGAL, "IO B={b}");
        assert_eq!(fault(&isa::wide(op::IOI, b, 1, 0, 0)), vector::ILLEGAL, "IOI B={b}");
    }
    for b in [0, 5, -1, -3] {
        assert_eq!(fault(&isa::fld(b, 1, 2, 1, 1)), vector::ILLEGAL, "FLD B={b}");
    }
    for b in [11, 12, -1, -4] {
        assert_eq!(fault(&[rrr(op::SYS, b, 0, 0, 0)]), vector::ILLEGAL, "SYS B={b}");
    }
}

#[test]
fn bad_trit_fields_are_illegal() {
    for (pos, len) in [(0, 1), (1, 0), (-3, 2), (27, 2), (20, 9), (1, 28)] {
        assert_eq!(fault(&isa::fld(fld::EXT, 1, 2, pos, len)), vector::ILLEGAL, "pos {pos} len {len}");
        assert_eq!(fault(&isa::fld(fld::INS, 1, 2, pos, len)), vector::ILLEGAL, "pos {pos} len {len}");
    }
    // The edges are fine.
    assert_eq!(fault(&isa::fld(fld::EXT, 1, 2, 1, 27)), NO_FAULT);
    assert_eq!(fault(&isa::fld(fld::EXT, 1, 2, 27, 1)), NO_FAULT);
}

#[test]
fn bad_bit_fields_are_illegal() {
    for (pos, len) in [(-1, 8), (81, 8), (0, 0), (0, 42), (5, -1)] {
        assert_eq!(fault(&isa::fld(fld::BEXT, 1, 2, pos, len)), vector::ILLEGAL, "BEXT pos {pos} len {len}");
    }
    for (pos, len) in [(-1, 8), (8, 8), (9, 8), (0, 0), (0, 42), (41, 41)] {
        assert_eq!(fault(&isa::fld(fld::BROT, 1, 2, pos, len)), vector::ILLEGAL, "BROT pos {pos} len {len}");
    }
    // The edges are fine.
    for (pos, len) in [(0, 1), (80, 1), (0, 41), (80, 41)] {
        assert_eq!(fault(&isa::fld(fld::BEXT, 1, 2, pos, len)), NO_FAULT, "BEXT pos {pos} len {len}");
    }
    for (pos, len) in [(0, 1), (0, 41), (40, 41), (31, 32)] {
        assert_eq!(fault(&isa::fld(fld::BROT, 1, 2, pos, len)), NO_FAULT, "BROT pos {pos} len {len}");
    }
}

#[test]
fn unassigned_major_opcodes_are_illegal() {
    for a in [12, 100, 3280, -1, -3280] {
        assert_eq!(fault(&[rrr(a, 1, 0, 0, 0)]), vector::ILLEGAL, "1-word A={a}");
    }
    for a in [6570, 6560 + 3281, 3281, 9841] {
        assert_eq!(fault(&isa::wide(a, 1, 0, 0, 0)), vector::ILLEGAL, "2-word A={a}");
    }
    for a in [-3281, -6561, -9841] {
        assert_eq!(fault(&[rrr(a, 0, 0, 0, 0)]), vector::ILLEGAL, "reserved A={a}");
    }
}

#[test]
fn mixed_branch_condition_is_illegal() {
    let mixed = 9 - 1; // "10T": S<0 or C>0 — reserved
    assert_eq!(fault(&[isa::s6(op::BRQ, 0, mixed, 5)]), vector::ILLEGAL);
    assert_eq!(fault(&isa::wide(op::BR, 0, mixed, 0, 5)), vector::ILLEGAL);
    // `000` is "never": falls through, no fault.
    let mut m = machine();
    assert_eq!(run(&mut m, &[isa::s6(op::BRQ, 0, cond::NEVER, 5), liq(1, 9)]), NO_FAULT);
    assert_eq!(m.cpu.reg(1).value(), 9);
}

#[test]
fn illegal_instruction_reports_its_own_address() {
    let mut m = machine();
    assert_eq!(run(&mut m, &[liq(1, 1), liq(2, 2), Word::ZERO]), vector::ILLEGAL);
    assert_eq!(m.cpu.cr(cr::EPC).value(), 6);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(21..=21), 1, "raised in ring +1");
}

// ---- privilege -------------------------------------------------------------

#[test]
fn kernel_only_instructions_trap_in_rings_minus1_and_0() {
    let kernel_only = [
        rrr(op::SYS, sys::HLT, 0, 0, 0),
        rrr(op::SYS, sys::IRET, 0, 0, 0),
        rrr(op::SYS, sys::MFC, 1, cr::MODE, 0),
        rrr(op::SYS, sys::MTC, cr::SCRATCH, 1, 0),
        rrr(op::SYS, sys::TLBINV, 0, 1, 0),
        rrr(op::SYS, sys::TLBFLUSH, 0, 0, 0),
    ];
    for ring in [-1, 0] {
        for (i, inst) in kernel_only.iter().enumerate() {
            assert_eq!(in_ring(ring, &[*inst]), vector::PRIVILEGE, "ring {ring}, instruction {i}");
        }
    }
    // The unprivileged SYS-group members work in user mode.
    assert_eq!(in_ring(-1, &[rrr(op::SYS, sys::NOP, 0, 0, 0), rrr(op::SYS, sys::FENCE, 0, 0, 0)]), vector::PRIVILEGE,
        "…until the final HLT, which is itself privileged");
}

#[test]
fn iopl_gates_port_io() {
    let io_in = rrr(op::IO, isa::io::IN, 1, 0, 0);
    let io_out = isa::wide(op::IOI, isa::io::OUT, 1, 0, 3);
    // Default IOPL 0: user mode blocked, drivers allowed.
    assert_eq!(in_ring(-1, &[io_in]), vector::PRIVILEGE);
    assert_eq!(in_ring(-1, &io_out), vector::PRIVILEGE);
    let mut m = machine();
    m.cpu.set_mode(mode::RING, 0);
    assert_eq!(run(&mut m, &[io_in]), vector::PRIVILEGE, "the final HLT traps, not the IN");
    assert_eq!(m.cpu.cr(cr::EPC).value(), 3, "IN itself was allowed in ring 0");
    // IOPL +1: drivers blocked too.
    let mut m = machine();
    m.cpu.set_mode(mode::RING, 0);
    m.cpu.set_mode(mode::IOPL, 1);
    assert_eq!(run(&mut m, &[io_in]), vector::PRIVILEGE);
    assert_eq!(m.cpu.cr(cr::EPC).value(), 0, "IN trapped");
    // IOPL −1: even user mode may do I/O.
    let mut m = machine();
    m.cpu.set_mode(mode::RING, -1);
    m.cpu.set_mode(mode::IOPL, -1);
    run(&mut m, &[io_in]);
    assert_eq!(m.cpu.cr(cr::EPC).value(), 3, "IN allowed, final HLT trapped");
}

#[test]
fn read_only_and_reserved_control_registers() {
    for n in [cr::CPUID, cr::CYCLE, 11, 13, -1, -13] {
        assert_eq!(fault(&[rrr(op::SYS, sys::MTC, n, 0, 0)]), vector::ILLEGAL, "MTC cr{n}");
    }
    let mut m = machine();
    assert_eq!(run(&mut m, &[liq(1, 5), rrr(op::SYS, sys::MFC, 1, -7, 0)]), NO_FAULT);
    assert_eq!(m.cpu.reg(1).value(), 0, "reserved CRs read as 0");
    assert_eq!(m.cpu.cr(cr::CPUID).value(), setun_core::cpu::CPUID, "CPUID untouched");
}

// ---- alignment and bus errors ----------------------------------------------

#[test]
fn misaligned_accesses() {
    for addr in [1, 2, 301, -1, -302] {
        for (b, acc) in [(mem::LDW, Access::Read), (mem::STW, Access::Write), (mem::SWAP, Access::Read)] {
            let mut m = machine();
            let prog = cat([&li(1, addr), &[rrr(op::MEM, b, 2, 1, 0)]]);
            assert_eq!(run(&mut m, &prog), vector::MISALIGNED, "B={b} at {addr}");
            assert_eq!(m.cpu.cr(cr::FADDR).value(), addr);
            assert_eq!(m.cpu.cr(cr::FCAUSE).field(22..=22), acc as i64);
        }
        // Tryte accesses have no alignment rule.
        assert_eq!(fault(&cat([&li(1, addr), &[rrr(op::MEM, mem::LDT, 2, 1, 0)]])), NO_FAULT);
    }
}

#[test]
fn jump_to_misaligned_address_faults_on_fetch() {
    let mut m = machine();
    assert_eq!(run(&mut m, &[liq(1, 31), rrr(op::JALR, 0, 0, 1, 0)]), vector::MISALIGNED);
    assert_eq!(m.cpu.cr(cr::EPC).value(), 31);
    assert_eq!(m.cpu.cr(cr::FADDR).value(), 31);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(22..=22), Access::Fetch as i64);
}

#[test]
fn bus_errors_on_unmapped_physical_memory() {
    let nowhere = frame_start(10); // RAM is frames 0..=1
    for (b, acc) in [(mem::LDW, Access::Read), (mem::STW, Access::Write), (mem::LDT, Access::Read), (mem::STT, Access::Write)] {
        let mut m = machine();
        let prog = cat([&li(1, nowhere + 1), &[rrr(op::MEM, b, 2, 1, 0)]]);
        assert_eq!(run(&mut m, &prog), vector::BUS_ERROR, "B={b}");
        assert_eq!(m.cpu.cr(cr::FCAUSE).field(22..=22), acc as i64);
    }
    // Writing ROM is a bus error; reading it is fine.
    let rom = -59_049;
    assert_eq!(fault(&cat([&li(1, rom), &[rrr(op::MEM, mem::STW, 2, 1, 0)]])), vector::BUS_ERROR);
    assert_eq!(fault(&cat([&li(1, rom), &[rrr(op::MEM, mem::LDW, 2, 1, 0)]])), NO_FAULT);
    // Jumping into the void faults on the fetch.
    let mut m = machine();
    assert_eq!(run(&mut m, &cat([&li(1, nowhere + 1), &[rrr(op::JALR, 0, 0, 1, 0)]])), vector::BUS_ERROR);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(22..=22), Access::Fetch as i64);
}

// ---- faults have no side effects -------------------------------------------

#[test]
fn faulting_instructions_change_nothing() {
    // DIV / MOD / DIVR / MODR by zero keep rd and the flags.
    for f in [alu::DIV, alu::MOD, alu::DIVR, alu::MODR] {
        let mut m = machine();
        let prog = [liq(1, 7), liq(2, -5), rrr(op::ALU, alu::ADD, 3, 1, 2), rrr(op::ALU, f, 2, 1, 0)];
        assert_eq!(run(&mut m, &prog), vector::DIVIDE, "f={f}");
        assert_eq!(m.cpu.reg(2).value(), -5, "rd untouched");
        assert_eq!(m.cpu.cr(cr::EMODE).trit(mode::S) as i64, 1, "flags from the ADD survive");
    }
    // A store into ROM leaves memory and the register alone; SWAP too.
    let mut m = machine();
    let rom = -59_049;
    let before = m.mem.read_word(rom).unwrap();
    let prog = cat([&li(1, rom), &[liq(2, 77), rrr(op::MEM, mem::SWAP, 2, 1, 0)]]);
    assert_eq!(run(&mut m, &prog), vector::BUS_ERROR);
    assert_eq!(m.mem.read_word(rom).unwrap(), before);
    assert_eq!(m.cpu.reg(2).value(), 77);
    // A word store that straddles the end of RAM writes nothing.
    let mut mem = Memory::new(1);
    mem.write_tryte(9840, Tryte::new(5).unwrap()).unwrap();
    assert_eq!(mem.write_word(9841, Word::MAX), Err(BusError));
    assert_eq!(mem.read_tryte(9840).unwrap().value(), 5);
    assert_eq!(mem.read_tryte(9841).unwrap().value(), 0);
}

#[test]
fn double_fault_when_only_the_vector_slot_is_missing() {
    // ITB placed so the ILLEGAL slot (ITB − 3) lies past the end of RAM
    // while the DOUBLE_FAULT slot (ITB − 27) is still inside it.
    let mut m = Machine::new(1, 0);
    let itb = 9855;
    m.mem.write_word(itb - 27, w(HANDLER)).unwrap();
    m.mem.load_words(HANDLER, &[isa::rrr(op::SYS, sys::HLT, 0, 0, 0)]);
    m.cpu.set_cr(cr::ITB, w(itb));
    m.mem.load_words(0, &[liq(1, 1), Word::ZERO]);
    m.cpu.pc = Word::ZERO;
    m.run(10);
    assert_eq!(m.cpu.pc.value(), HANDLER + 3);
    assert_eq!(m.cpu.cr(cr::FCAUSE).field(23..=27), vector::DOUBLE_FAULT);
    assert_eq!(m.cpu.cr(cr::EPC).value(), 3, "EPC still names the original fault");
}

// ---- arithmetic edge cases -------------------------------------------------

#[test]
fn arithmetic_edges() {
    let mut m = machine();
    let prog = cat([
        &li(1, Word::MIN.value()),
        &[rrr(op::ALUQ, alu::SUB, 2, 1, 1)], // MIN − 1 → MAX, C = −1
    ]);
    run(&mut m, &prog);
    assert_eq!(m.cpu.reg(2), Word::MAX);
    assert_eq!(m.cpu.c(), -1);

    let prog = cat([
        &li(1, Word::MIN.value()),
        &[liq(2, 3), rrr(op::ALU, alu::MUL, 3, 1, 2)], // negative overflow
    ]);
    run(&mut m, &prog);
    assert_eq!(m.cpu.c(), -1);

    // CMP is exact: MAX vs MIN is "greater", although MAX − MIN overflows.
    let prog = cat([&li(1, Word::MAX.value()), &li(2, Word::MIN.value()), &[rrr(op::ALU, alu::CMP, 0, 1, 2)]]);
    run(&mut m, &prog);
    assert_eq!(m.cpu.s(), 1);

    // TRIT outside 1..=27 is 0; SH past the width is 0.
    for pos in [0, 28, -5, 1000] {
        let prog = cat([&li(1, Word::MAX.value()), &isa::wide(op::ALUI, alu::TRIT, 2, 1, pos)]);
        run(&mut m, &prog);
        assert_eq!(m.cpu.reg(2).value(), 0, "TRIT {pos}");
    }
    for n in [27, 1000, -28, -1000] {
        let prog = cat([&li(1, Word::MAX.value()), &isa::wide(op::ALUI, alu::SH, 2, 1, n)]);
        run(&mut m, &prog);
        assert_eq!(m.cpu.reg(2).value(), 0, "SH {n}");
    }
    // ABS(MIN) = MAX: balanced ternary has no asymmetric minimum.
    let prog = cat([&li(1, Word::MIN.value()), &[rrr(op::ALU, alu::ABS, 2, 1, 0)]]);
    run(&mut m, &prog);
    assert_eq!(m.cpu.reg(2), Word::MAX);
}

// ---- MMU -------------------------------------------------------------------

const L1: i64 = 59_049;
const L2_FRAME: i64 = 6;

fn pte(frame: i64, v: i64, x: i64, ring: i64) -> Word {
    Word::ZERO.with_field(1..=18, frame).with_field(19..=19, v).with_field(20..=20, x).with_field(21..=21, ring)
}

fn paged(e1: Word, e2: Word, ring: i64) -> (Memory, setun_core::Cpu) {
    let mut mem = Memory::new(27);
    mem.write_word(L1, e1).unwrap();
    mem.write_word(L2_FRAME * PAGE, e2).unwrap();
    let mut cpu = setun_core::Cpu::new();
    cpu.set_cr(cr::PTB, w(L1));
    cpu.set_mode(mode::PG, 1);
    cpu.set_mode(mode::RING, ring);
    (mem, cpu)
}

fn vec_of(r: Result<i64, setun_core::cpu::Fault>) -> i64 {
    r.map(|_| NO_FAULT).unwrap_or_else(|f| f.vector)
}

#[test]
fn mmu_permission_matrix() {
    use Access::*;
    let l1_open = pte(L2_FRAME, 1, 1, -1);
    let cases = [
        // (E2 V, E2 X, E2 RING, access, ring, expected)
        (1, 0, -1, Fetch, -1, vector::PROTECTION),  // no-exec
        (1, -1, -1, Read, -1, vector::PROTECTION),  // execute-only
        (1, -1, -1, Fetch, -1, NO_FAULT),
        (-1, 1, -1, Write, -1, vector::PROTECTION), // read-only
        (-1, 1, -1, Read, -1, NO_FAULT),
        (1, 1, 0, Read, -1, vector::PROTECTION),    // drivers' page, user access
        (1, 1, 0, Read, 0, NO_FAULT),
        (1, 1, 1, Write, 0, vector::PROTECTION),    // kernel page, driver access
        (1, 1, 1, Write, 1, NO_FAULT),
        (0, 1, -1, Read, 1, vector::PAGE_FAULT),    // not present, even for the kernel
    ];
    for (v, x, r, acc, ring, expect) in cases {
        let (mut mem, cpu) = paged(l1_open, pte(10, v, x, r), ring);
        assert_eq!(vec_of(cpu.translate(&mut mem, 0, acc)), expect, "V{v} X{x} R{r} {acc:?} ring {ring}");
    }
}

#[test]
fn mmu_most_restrictive_level_wins() {
    use Access::*;
    let e2_open = pte(10, 1, 1, -1);
    // L1 read-only → no writes anywhere below it.
    let (mut mem, cpu) = paged(pte(L2_FRAME, -1, 1, -1), e2_open, -1);
    assert_eq!(vec_of(cpu.translate(&mut mem, 0, Write)), vector::PROTECTION);
    // L1 no-exec → no fetches.
    let (mut mem, cpu) = paged(pte(L2_FRAME, 1, 0, -1), e2_open, -1);
    assert_eq!(vec_of(cpu.translate(&mut mem, 0, Fetch)), vector::PROTECTION);
    // L1 kernel-only → user locked out.
    let (mut mem, cpu) = paged(pte(L2_FRAME, 1, 1, 1), e2_open, -1);
    assert_eq!(vec_of(cpu.translate(&mut mem, 0, Read)), vector::PROTECTION);
    // L1 absent → page fault before L2 is even looked at.
    let (mut mem, cpu) = paged(Word::ZERO, e2_open, 1);
    assert_eq!(vec_of(cpu.translate(&mut mem, 0, Read)), vector::PAGE_FAULT);
}

#[test]
fn mmu_denied_access_leaves_accessed_dirty_alone() {
    let (mut mem, cpu) = paged(pte(L2_FRAME, 1, 1, -1), pte(10, -1, 1, -1), -1);
    assert!(cpu.translate(&mut mem, 0, Access::Write).is_err());
    assert_eq!(mem.read_word(L2_FRAME * PAGE).unwrap().trit(22) as i64, 0);
}

#[test]
fn mmu_walk_into_the_void_is_a_bus_error() {
    // PTB pointing at nothing.
    let (mut mem, mut cpu) = paged(Word::ZERO, Word::ZERO, 1);
    cpu.set_cr(cr::PTB, w(frame_start(100) + 1));
    let f = cpu.translate(&mut mem, 0, Access::Read).unwrap_err();
    assert_eq!((f.vector, f.addr), (vector::BUS_ERROR, 0));
    // L1 entry naming a nonexistent L2 frame.
    let (mut mem, cpu) = paged(pte(500, 1, 1, -1), Word::ZERO, 1);
    assert_eq!(vec_of(cpu.translate(&mut mem, 0, Access::Read)), vector::BUS_ERROR);
    // Valid walk to a frame with no memory: translation succeeds; the access
    // itself bus-errors (checked by the CPU tests).
    let (mut mem, cpu) = paged(pte(L2_FRAME, 1, 1, -1), pte(500, 1, 1, -1), 1);
    assert_eq!(cpu.translate(&mut mem, 3, Access::Read), Ok(500 * PAGE + 3));
}

// ---- memory map ------------------------------------------------------------

#[test]
fn memory_map_holes() {
    let mut m = Memory::new(2);
    for pa in [
        frame_start(2),              // past RAM
        frame_start(-4),             // below ROM
        frame_start(-54),            // between framebuffer and ROM
        frame_start(-55) - 1,        // just below framebuffer top… (frame −56 is FB)
        frame_start(-82),            // below framebuffer
        Word::MAX.value(),
        Word::MIN.value(),
    ] {
        if pa == frame_start(-55) - 1 {
            assert!(m.read_tryte(pa).is_ok(), "frame −56 is framebuffer");
            continue;
        }
        assert_eq!(m.read_tryte(pa), Err(BusError), "read {pa}");
        assert_eq!(m.write_tryte(pa, Tryte::MAX), Err(BusError), "write {pa}");
    }
}

#[test]
#[should_panic(expected = "at least one RAM frame")]
fn memory_needs_ram() {
    Memory::new(0);
}

#[test]
#[should_panic(expected = "poke outside the memory map")]
fn poke_outside_map_panics() {
    Memory::new(1).poke(frame_start(5), Tryte::MAX);
}

#[test]
#[should_panic(expected = "ROM image too large")]
fn oversized_rom_panics() {
    Machine::new(1, 0).load_rom(&vec![0; 3 * PAGE as usize + 1]);
}

// ---- devices ---------------------------------------------------------------

fn port(b: i64, r: i64) -> i64 {
    27 * b + r
}

fn out(m: &mut Machine, b: i64, r: i64, v: i64) {
    m.dev.port_out(port(b, r), w(v));
}

fn inp(m: &mut Machine, b: i64, r: i64) -> i64 {
    m.dev.port_in(port(b, r)).value()
}

#[test]
fn unknown_ports_read_zero_and_ignore_writes() {
    let mut m = machine();
    for p in [port(13, 0), port(-1, 0), port(364, 5), 9841, -9841, port(block::SYSCTL, 13), port(block::DISPLAY, -13)] {
        m.dev.port_out(p, Word::MAX);
        assert_eq!(m.dev.port_in(p).value(), 0, "port {p}");
    }
    assert_eq!(m.dev.pending_vector(), None);
    assert!(m.dev.take_power().is_none());
}

#[test]
fn sysctl_ignores_bogus_power_values() {
    let mut m = machine();
    for v in [0, 2, -2, 364] {
        out(&mut m, block::SYSCTL, 2, v);
    }
    assert!(m.dev.take_power().is_none());
    out(&mut m, block::SYSCTL, 0, 999); // SYSID is read-only
    assert_eq!(inp(&mut m, block::SYSCTL, 0), 26);
}

#[test]
fn pic_ignores_out_of_range_lines() {
    let mut m = machine();
    for v in [-1, 81, 1000, -9841] {
        out(&mut m, block::TPIC, 3, v); // UNMASK
        out(&mut m, block::TPIC, 2, v); // EOI
        out(&mut m, block::TPIC, 5, v); // SELECT
        assert_eq!(inp(&mut m, block::TPIC, 6), 0, "STATE of line {v}");
    }
    assert_eq!(inp(&mut m, block::TPIC, 1), -1, "nothing to claim");
    // A pending but masked line never interrupts, and re-masking silences it.
    m.dev.pic.raise(3);
    assert_eq!(m.dev.pending_vector(), None);
    out(&mut m, block::TPIC, 3, 3);
    assert!(m.dev.pending_vector().is_some());
    out(&mut m, block::TPIC, 4, 3);
    assert_eq!(m.dev.pending_vector(), None);
    // EOI for a line that is not in service is harmless.
    out(&mut m, block::TPIC, 2, 7);
}

#[test]
fn interrupts_stay_queued_while_ie_is_off() {
    let mut m = machine();
    out(&mut m, block::TPIC, 3, 0);
    m.dev.pic.raise(0);
    m.mem.load_words(0, &[liq(1, 1), liq(2, 2)]);
    m.cpu.pc = Word::ZERO;
    m.step();
    m.step();
    assert_eq!(m.cpu.reg(2).value(), 2, "ran on, interrupt not taken");
    assert!(m.dev.pending_vector().is_some(), "still pending");
}

#[test]
fn timer_misconfiguration_never_fires() {
    let mut m = machine();
    out(&mut m, block::TPIC, 3, 0);
    out(&mut m, block::TIMER, 1, 0); // zero period
    out(&mut m, block::TIMER, 0, 1);
    m.dev.advance_us(1_000_000);
    assert_eq!(m.dev.pending_vector(), None);
    out(&mut m, block::TIMER, 1, -500); // negative period clamps to 0
    assert_eq!(inp(&mut m, block::TIMER, 1), 0);
    out(&mut m, block::TIMER, 1, 10);
    out(&mut m, block::TIMER, 0, 0); // stopped
    m.dev.advance_us(1000);
    assert_eq!(m.dev.pending_vector(), None);
    out(&mut m, block::TIMER, 0, 77); // any positive value means periodic
    assert_eq!(inp(&mut m, block::TIMER, 0), 1);
}

#[test]
fn rtc_alarm_edge_cases() {
    let mut m = machine();
    out(&mut m, block::TPIC, 3, 5);
    m.dev.advance_us(10_000_000);
    assert_eq!(m.dev.pending_vector(), None, "alarm 0 means off");
    out(&mut m, block::RTC, 2, 3); // already in the past
    m.dev.advance_us(1);
    assert!(m.dev.pending_vector().is_some(), "a past alarm fires at once");
    out(&mut m, block::RTC, 1, 5); // MICROS is read-only
    assert_eq!(inp(&mut m, block::RTC, 1), 1);
}

#[test]
fn input_devices_when_empty_or_disabled() {
    let mut m = machine();
    for line in [1, 2, 3] {
        out(&mut m, block::TPIC, 3, line);
    }
    assert_eq!(inp(&mut m, block::KEYBOARD, 0), 0, "empty queue reads 0");
    assert_eq!(inp(&mut m, block::SERIAL, 0), 0);
    assert_eq!(inp(&mut m, block::MOUSE, 3), 0);
    // IRQs off: events queue silently.
    m.dev.key(5, true);
    m.dev.mouse_move(3, 3);
    m.dev.mouse_buttons(true, false, false);
    m.dev.serial_input("x");
    assert_eq!(m.dev.pending_vector(), None);
    // Flush drops queued keys; an unknown CTRL value changes nothing.
    out(&mut m, block::KEYBOARD, 2, -1);
    assert_eq!(inp(&mut m, block::KEYBOARD, 1), 0);
    out(&mut m, block::KEYBOARD, 2, 7);
    assert_eq!(inp(&mut m, block::KEYBOARD, 2), 0);
    // IRQ on, but no change: no interrupt.
    out(&mut m, block::MOUSE, 4, 1);
    m.dev.mouse_move(0, 0);
    m.dev.mouse_buttons(true, false, false); // same as before
    m.dev.mouse_wheel(0);
    assert_eq!(m.dev.pending_vector(), None);
    // Output of values that are not characters becomes U+FFFD: a surrogate,
    // and negatives down to -32 (overlined control characters mean nothing),
    // also -0xD800 (an overlined surrogate).
    for v in [-1, -32, 0xD800, -0xD800] {
        out(&mut m, block::SERIAL, 0, v);
    }
    assert_eq!(m.dev.serial.take_output(), "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{305}");
    // a negative code point below -32 is the character overlined
    for v in [-('5' as i64), -9841, -33] {
        out(&mut m, block::SERIAL, 0, v);
    }
    assert_eq!(m.dev.serial.take_output(), "5\u{305}\u{2671}\u{305}!\u{305}");
}

#[test]
fn serial_input_folds_overlines() {
    let mut m = machine();
    out(&mut m, block::SERIAL, 3, 1); // receive interrupts on
    // digit + U+0305 arrives as one negative code point; a lone or doubled
    // overline, or one after a control character, changes nothing
    m.dev.serial_input("0z1D\u{305}\u{305}x \u{305}\n\u{305}");
    let mut got = Vec::new();
    while inp(&mut m, block::SERIAL, 1) > 0 {
        got.push(inp(&mut m, block::SERIAL, 0));
    }
    assert_eq!(got, vec!['0' as i64, 'z' as i64, '1' as i64, 'D' as i64, 'x' as i64, ' ' as i64, '\n' as i64]
        .into_iter()
        .enumerate()
        .map(|(i, c)| if i == 3 { -c } else { c })
        .collect::<Vec<_>>());
}

#[test]
fn serial_carries_characters_above_one_tryte() {
    // A6: typed in as 3 trytes (an overline after one turns its lead to the
    // overlined lead); written out, they come back whole even when a drain
    // cuts one; malformed ones are U+FFFD
    let mut m = machine();
    m.dev.serial_input("日\u{305}😀x\u{305}");
    let mut got = Vec::new();
    while inp(&mut m, block::SERIAL, 1) > 0 {
        got.push(inp(&mut m, block::SERIAL, 0));
    }
    let mut want = ternary::char_trytes(0x65E5, true);
    want.extend(ternary::char_trytes(0x1F600, false));
    want.push(-('x' as i64));
    assert_eq!(got, want);
    for &t in &want[..4] {
        out(&mut m, block::SERIAL, 0, t);
    }
    assert_eq!(m.dev.serial.take_output(), "日\u{305}", "the second character is cut off");
    for &t in &want[4..] {
        out(&mut m, block::SERIAL, 0, t);
    }
    assert_eq!(m.dev.serial.take_output(), "😀x\u{305}");
    for t in [-1, 65, -2, 66, -4, -2, -9841, -9841] {
        out(&mut m, block::SERIAL, 0, t);
    }
    assert_eq!(m.dev.serial.take_output(), "\u{FFFD}A\u{FFFD}B\u{FFFD}\u{FFFD}");
}

#[test]
fn disk_error_paths() {
    let mut m = machine();
    let run_cmd = |m: &mut Machine, cmd: i64| {
        out(m, block::DISK0, 0, cmd);
        m.dev.service(&mut m.mem);
        (inp(m, block::DISK0, 1), inp(m, block::DISK0, 6))
    };
    // No media at all.
    assert_eq!(run_cmd(&mut m, -1), (-1, 1));
    assert_eq!(run_cmd(&mut m, 0), (-1, 1), "flush without media");
    assert_eq!(inp(&mut m, block::DISK0, 5), 0, "SECTORS");

    m.dev.disks[0].attach(Box::new(MemDisk::new(2)));
    out(&mut m, block::DISK0, 4, frame_start(1));
    out(&mut m, block::DISK0, 3, 1);
    // Bad command codes.
    for cmd in [2, -2, 364] {
        assert_eq!(run_cmd(&mut m, cmd), (-1, 6), "cmd {cmd}");
    }
    // Negative LBA / count, and a range past the end.
    for (lba, count) in [(-1, 1), (0, -1), (1, 2), (2, 1)] {
        out(&mut m, block::DISK0, 2, lba);
        out(&mut m, block::DISK0, 3, count);
        assert_eq!(run_cmd(&mut m, -1), (-1, 2), "lba {lba} count {count}");
    }
    // DMA running off the end of RAM (frames 0..=1): sector 2 has no frame.
    out(&mut m, block::DISK0, 2, 0);
    out(&mut m, block::DISK0, 3, 2);
    out(&mut m, block::DISK0, 4, frame_start(1));
    assert_eq!(run_cmd(&mut m, -1), (-1, 4));
    // DMA source nowhere (write from a hole).
    out(&mut m, block::DISK0, 3, 1);
    out(&mut m, block::DISK0, 4, frame_start(-20));
    assert_eq!(run_cmd(&mut m, 1), (-1, 4));
    // Count 0 is a successful no-op; errors clear on the next good command.
    out(&mut m, block::DISK0, 3, 0);
    assert_eq!(run_cmd(&mut m, -1), (0, 0));
}

#[test]
fn disk_ignores_commands_while_busy() {
    let mut m = machine();
    m.dev.disks[0].attach(Box::new(MemDisk::new(1)));
    out(&mut m, block::DISK0, 3, 0);
    out(&mut m, block::DISK0, 0, -1);
    out(&mut m, block::DISK0, 0, 5); // bogus, but ignored: still busy
    assert_eq!(inp(&mut m, block::DISK0, 0), -1, "the first command stands");
    m.dev.service(&mut m.mem);
    assert_eq!(inp(&mut m, block::DISK0, 1), 0);
}

#[test]
fn file_disk_failures() {
    let missing = std::env::temp_dir().join("setun26-definitely-missing.t1b2");
    assert!(FileDisk::open(&missing).is_err());
    // A truncated image only exposes whole sectors.
    let path = std::env::temp_dir().join(format!("setun26-trunc-{}.t1b2", std::process::id()));
    std::fs::write(&path, vec![0u8; 2 * PAGE as usize + 100]).unwrap();
    let d = FileDisk::open(&path).unwrap();
    assert_eq!(d.sectors(), 1);
    drop(d);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn display_tolerates_odd_values() {
    let mut m = machine();
    out(&mut m, block::DISPLAY, 0, 5); // any positive = 800×600
    assert_eq!(inp(&mut m, block::DISPLAY, 0), 1);
    out(&mut m, block::DISPLAY, 0, -99);
    assert_eq!(inp(&mut m, block::DISPLAY, 0), -1);
    out(&mut m, block::DISPLAY, 1, 0); // FB_BASE is read-only
    assert_eq!(inp(&mut m, block::DISPLAY, 1), -1_604_164);
    // Cursor and characters out of range, mouse cursor off-screen: no panic.
    out(&mut m, block::DISPLAY, 7, 10_000);
    for (x, y) in [(-5, -5), (636, 476), (1_000_000, 3), (-1_000_000, -1_000_000)] {
        out(&mut m, block::DISPLAY, 8, x);
        out(&mut m, block::DISPLAY, 9, y);
        out(&mut m, block::DISPLAY, 10, 1);
        let base = setun_core::devices::Display::fb_base();
        m.mem.write_tryte(base, Tryte::MIN).unwrap(); // negative "character"
        m.mem.write_tryte(base + 3, Tryte::MAX).unwrap(); // code point with no glyph
        let mut buf = Vec::new();
        assert_eq!(m.dev.display.render(&m.mem, &mut buf), (640, 480));
    }
    // vsync with its IRQ disabled raises nothing.
    out(&mut m, block::TPIC, 3, 6);
    m.dev.advance_us(1_000_000);
    assert_eq!(m.dev.pending_vector(), None);
}

#[test]
fn powered_off_machine_does_nothing() {
    let mut m = machine();
    out(&mut m, block::SYSCTL, 2, -1);
    m.step();
    assert!(m.powered_off);
    let pc = m.cpu.pc;
    assert_eq!(m.step(), Step::Halted);
    assert_eq!(m.run(100), 0);
    assert_eq!(m.cpu.pc, pc);
}

#[test]
fn power_reset_keeps_ram() {
    let mut m = machine();
    m.mem.write_tryte(100, Tryte::new(42).unwrap()).unwrap();
    m.cpu.set_reg(5, w(9));
    out(&mut m, block::SYSCTL, 2, 1);
    m.step();
    assert_eq!(m.cpu.pc.value(), -59_049);
    assert_eq!(m.cpu.reg(5).value(), 0);
    assert_eq!(m.mem.read_tryte(100).unwrap().value(), 42);
}
