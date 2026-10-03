//! The fast execution path (batches, TLB, decoded-instruction cache, fetch
//! window) against the reference path (`Machine::step`, a full decode and
//! page walk every time): random programs, run both ways, must agree on the
//! whole machine state at every checkpoint. The programs are random but
//! mostly well-formed, so they fault, trap, take interrupts, write their own
//! code and their own page tables, and switch rings.

use setun_core::isa::{cr, mode, op};
use setun_core::{Machine, PAGE};
use ternary::Word;

const FRAMES: i64 = 9;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

fn w(v: i64) -> Word {
    Word::new(v).unwrap()
}

/// A random instruction word: a real major most of the time, a random
/// function, registers and offsets.
fn insn(r: &mut Rng) -> i64 {
    let majors = [
        op::SYS, op::ALU, op::ALUQ, op::MEM, op::IO, op::JALR, op::LIQ, op::BRQ, op::ALUI, op::MEMD, op::LI, op::LEA,
        op::JAL, op::BR, op::BR3, op::IOI, op::FLD, op::FALU, op::FCVT, op::FMEM, op::FMEMD, op::FLI,
    ];
    let a = if r.range(0, 20) == 0 { r.range(-9841, 9841) } else { r.pick(&majors) };
    let b = if r.range(0, 10) == 0 { r.range(-9841, 9841) } else { r.range(-3, 24) };
    let (x, y, z) = (r.range(-13, 13), r.range(-13, 13), r.range(-13, 13));
    (a * 19_683 + b) * 19_683 + x * 729 + y * 27 + z
}

/// A 2nd word: small offsets mostly (branches stay nearby), sometimes any.
fn imm(r: &mut Rng) -> i64 {
    match r.range(0, 4) {
        0 => r.range(-3_812_798_742_493, 3_812_798_742_493),
        1 => r.range(-6000, 6000) * 3,
        _ => r.range(-60, 60),
    }
}

fn pte(frame: i64, v: i64, x: i64, ring: i64) -> i64 {
    frame * 19_683 + v * 6561 + x * 2187 + ring * 729
}

fn machine(seed: u64) -> Machine {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut m = Machine::new(FRAMES, 1_790_000_000);
    // frames 0..2: code and data; every other word may be an instruction
    for f in 0..3 {
        let base = f * PAGE;
        let mut a = base - 9840;
        while a <= base + 9840 {
            let v = if r.range(0, 3) == 0 { imm(&mut r) } else { insn(&mut r) };
            m.mem.write_word(a, w(v)).unwrap();
            a += 3;
        }
    }
    // the vector table at frame 1's centre: handlers somewhere in frame 0
    let itb = PAGE;
    for v in -121..=121 {
        m.mem.write_word(itb + 3 * v, w(r.range(-3000, 3000) * 3)).unwrap();
    }
    m.cpu.set_cr(cr::ITB, w(itb));
    m.cpu.set_cr(cr::KSP, w(2 * PAGE + 3000));
    m.cpu.set_reg(13, w(2 * PAGE - 3000));
    // paging, half the time: L1 in frame 3, L2 in frame 4, pages 0..2 mapped
    // to frames 0..2 with random permissions (mostly open)
    if r.range(0, 1) == 0 {
        let l1 = 3 * PAGE;
        m.mem.write_word(l1, w(pte(4, 1, 1, -1))).unwrap();
        // pages 3 and 4 map the tables themselves, so programs can rewrite them
        m.mem.write_word(4 * PAGE + 9, w(pte(3, 1, 1, -1))).unwrap();
        m.mem.write_word(4 * PAGE + 12, w(pte(4, 1, 1, -1))).unwrap();
        for p in 0..3 {
            let (v, x, ring) = if r.range(0, 3) == 0 {
                (r.range(-1, 1), r.range(-1, 1), r.range(-1, 1))
            } else {
                (1, 1, -1)
            };
            m.mem.write_word(4 * PAGE + 3 * p, w(pte(p, v, x, ring))).unwrap();
        }
        m.cpu.set_cr(cr::PTB, w(l1));
        m.cpu.set_mode(mode::PG, 1);
    }
    if r.range(0, 2) == 0 {
        m.cpu.set_mode(mode::RING, -1);
    }
    if r.range(0, 2) == 0 {
        m.cpu.set_mode(mode::IE, 1);
    }
    // registers aimed at interesting words: the page tables, code, data
    for reg in 1..=12 {
        let target = match r.range(0, 4) {
            0 => 4 * PAGE + 3 * r.range(0, 4),
            1 => 3 * PAGE,
            2 => r.range(-3000, 3000) * 3,
            _ => r.range(-30000, 30000),
        };
        m.cpu.set_reg(reg, w(target));
    }
    m.cpu.pc = w(r.range(-3000, 3000) * 3);
    m
}

/// Everything observable.
fn digest(m: &mut Machine) -> Vec<i64> {
    let mut d = Vec::new();
    for r in -13..=13 {
        d.push(m.cpu.reg(r).value());
        d.push(m.cpu.cr(r).value());
        d.extend(m.cpu.freg(r).to_words());
    }
    d.push(m.cpu.pc.value());
    d.push(m.cpu.fsr().value());
    d.push(m.cpu.halted as i64);
    d.push(m.powered_off as i64);
    d.push(m.dev.now_us());
    for f in 0..FRAMES {
        let mut a = f * PAGE - 9840;
        while a <= f * PAGE + 9840 {
            d.push(m.mem.read_word(a).unwrap().value());
            a += 3;
        }
    }
    d.extend(m.dev.serial.take_output().chars().map(|c| c as i64));
    d
}

#[test]
fn random_programs_agree_with_the_reference_path() {
    let mut steps_total = 0;
    let mut jit = [0u64; 4];
    for seed in 1..=300u64 {
        let mut fast = machine(seed);
        let mut slow = machine(seed);
        // compile everything at once (odd seeds), or only what gets hot
        fast.cpu.set_jit_threshold(if seed % 2 == 1 { 1 } else { 3 });
        for chunk in 0..40 {
            let n = 1 + (seed * 7 + chunk) % 113;
            let ran = fast.run(n);
            let mut k = 0;
            while k < n && !slow.powered_off {
                slow.step();
                k += 1;
            }
            assert_eq!(ran, k, "seed {seed} chunk {chunk}: steps taken");
            steps_total += ran;
            let (a, b) = (digest(&mut fast), digest(&mut slow));
            if a != b {
                let i = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
                panic!("seed {seed} chunk {chunk}: state differs at item {i}: fast {:?} slow {:?}", a.get(i), b.get(i));
            }
            if fast.powered_off {
                break;
            }
        }
        for (t, v) in jit.iter_mut().zip(fast.cpu.jit_stats()) {
            *t += v;
        }
    }
    assert!(steps_total > 100_000, "the programs ran ({steps_total} steps)");
    #[cfg(not(target_arch = "wasm32"))]
    assert!(jit[0] > 100 && jit[2] > 30_000, "compiled code ran: {jit:?}");
    eprintln!("jit regions, entries, steps: {jit:?} of {steps_total}");
}

// ---- directed: the caches must notice changes right away ------------------

use setun_core::isa::{self, alu, cond, mem, sys};

/// Load `prog` at 0 and run it both ways for `steps`; the states must match.
fn both_ways(setup: impl Fn(&mut Machine), prog: &[Word], steps: u64) -> Vec<i64> {
    both_ways_jit(1, setup, prog, steps).0
}

/// `both_ways` with this JIT threshold on the fast side (`u32::MAX`: the
/// block interpreter only); also returns the fast machine.
fn both_ways_jit(threshold: u32, setup: impl Fn(&mut Machine), prog: &[Word], steps: u64) -> (Vec<i64>, Machine) {
    let mut fast = Machine::new(FRAMES, 0);
    let mut slow = Machine::new(FRAMES, 0);
    for m in [&mut fast, &mut slow] {
        let table: Vec<Word> = (-121..=121).map(|_| w(3 * 2900)).collect();
        m.mem.load_words(PAGE - 3 * 121, &table);
        m.mem.load_words(3 * 2900, &[isa::rrr(op::SYS, sys::HLT, 0, 0, 0)]);
        m.cpu.set_cr(cr::ITB, w(PAGE));
        m.mem.load_words(0, prog);
        m.cpu.pc = Word::ZERO;
        setup(m);
    }
    fast.cpu.set_jit_threshold(threshold);
    fast.run(steps);
    for _ in 0..steps {
        slow.step();
    }
    let (a, b) = (digest(&mut fast), digest(&mut slow));
    assert_eq!(a, b, "fast and reference paths differ");
    (a, fast)
}

/// Register `r` in a digest (five items per register: reg, cr, 3 FPU words).
fn reg_in(st: &[i64], r: i64) -> i64 {
    st[5 * (r + 13) as usize]
}

fn paged(m: &mut Machine) {
    m.mem.write_word(3 * PAGE, w(pte(4, 1, 1, -1))).unwrap();
    for p in 0..5 {
        m.mem.write_word(4 * PAGE + 3 * p, w(pte(p, 1, 1, -1))).unwrap();
    }
    m.cpu.set_cr(cr::PTB, w(3 * PAGE));
    m.cpu.set_mode(mode::PG, 1);
}

#[test]
fn unmapping_the_running_code_page_faults_on_the_next_fetch() {
    // r2 = the PTE of page 0 (seen at virtual 4·PAGE), r1 = 0; store, then
    // an instruction that touches no memory: it must not run
    let li = |r: i64, v: i64| isa::wide(op::LI, 0, r, 0, v);
    let mut prog = Vec::new();
    prog.extend(li(2, 4 * PAGE));
    prog.extend(li(1, 0));
    prog.push(isa::rrr(op::MEM, mem::STW, 1, 2, 0));
    prog.push(isa::s6(op::LIQ, 0, 5, 77));
    let st = both_ways(paged, &prog, 6);
    assert_ne!(reg_in(&st, 5), 77, "the instruction after the unmapping store must not execute");
}

#[test]
fn rewriting_an_instruction_that_already_ran_runs_the_new_one() {
    // LIQ r5, 1 runs once (and is decoded), then is overwritten with
    // LIQ r5, 2 and run again
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 1, 0, isa::s6(op::LIQ, 0, 5, 2).value())); // 0, 3
    prog.push(isa::s6(op::LIQ, 0, 5, 1)); // 6: the target
    prog.push(isa::rrr(op::ALUQ, alu::ADD, 6, 6, 1)); // 9: r6 += 1
    prog.push(isa::rrr(op::ALUQ, alu::CMP, 0, 6, 2)); // 12: r6 <=> 2
    prog.push(isa::s6(op::BRQ, 0, cond::GE, 4)); // 15: second time round -> 27
    prog.push(isa::rrr(op::MEM, mem::STW, 1, 0, 2)); // 18: [6] = r1
    prog.extend(isa::wide(op::JAL, 0, 0, 0, -5)); // 21, 24: back to 6
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0)); // 27
    for setup in [(|_: &mut Machine| {}) as fn(&mut Machine), paged] {
        let st = both_ways(setup, &prog, 20);
        assert_eq!(reg_in(&st, 5), 2, "the rewritten instruction ran");
        assert_eq!(reg_in(&st, 6), 2, "twice round the loop");
    }
}

#[test]
fn switching_page_tables_takes_effect_at_once() {
    // a second L1 (frame 5) maps page 0 to frame 2, where the code differs
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 1, 0, 5 * PAGE));
    prog.push(isa::rrr(op::SYS, sys::MTC, cr::PTB, 1, 0));
    prog.push(isa::s6(op::LIQ, 0, 5, 1)); // never reached through the new tables
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    let setup = |m: &mut Machine| {
        paged(m);
        m.mem.write_word(5 * PAGE, w(pte(6, 1, 1, -1))).unwrap();
        m.mem.write_word(6 * PAGE, w(pte(2, 1, 1, -1))).unwrap();
        // frame 2, at the address after the MTC: LIQ r5, 9 ; HLT
        m.mem.load_words(2 * PAGE - 9840 + 9840 + 9, &[isa::s6(op::LIQ, 0, 5, 9), isa::rrr(op::SYS, sys::HLT, 0, 0, 0)]);
    };
    let st = both_ways(setup, &prog, 10);
    assert_eq!(reg_in(&st, 5), 9, "fetched through the new tables");
    let _ = alu::ADD;
}

#[test]
fn remapping_a_data_page_takes_effect_at_once() {
    // load from page 1 (frame 1), remap page 1 to frame 2, load again
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 2, 0, PAGE + 300)); // r2 = an address in page 1
    prog.push(isa::rrr(op::MEM, mem::LDW, 5, 2, 0)); // r5 = [r2]
    prog.extend(isa::wide(op::LI, 0, 3, 0, 4 * PAGE + 3)); // r3 = the PTE of page 1
    prog.extend(isa::wide(op::LI, 0, 1, 0, pte(2, 1, 1, -1)));
    prog.push(isa::rrr(op::MEM, mem::STW, 1, 3, 0)); // page 1 -> frame 2
    prog.push(isa::rrr(op::MEM, mem::LDW, 6, 2, 0)); // r6 = [r2], now frame 2
    prog.push(isa::rrr(op::MEM, mem::STW, 6, 2, 1)); // and a store through it
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    let setup = |m: &mut Machine| {
        paged(m);
        m.mem.write_word(PAGE + 300, w(111)).unwrap();
        m.mem.write_word(2 * PAGE + 300, w(222)).unwrap();
    };
    let st = both_ways(setup, &prog, 12);
    assert_eq!((reg_in(&st, 5), reg_in(&st, 6)), (111, 222));
}

#[test]
fn carries_both_ways_in_a_compiled_loop() {
    // r1 = the smallest word, r2 = the largest; each round: r3 = r1 + r6
    // (r6 = -5: underflows, C = -1), r4 = C via ADC, r5 = r2 + 7
    // (overflows, C = +1), r7 += C via ADC, r8 counts rounds with C = +1
    // seen by a branch; 40 rounds
    let w_max = 3_812_798_742_493;
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 1, 0, -w_max));
    prog.extend(isa::wide(op::LI, 0, 2, 0, w_max));
    prog.push(isa::s6(op::LIQ, 0, 6, -5));
    prog.push(isa::s6(op::LIQ, 0, 9, 40));
    let top = prog.len() as i64;
    prog.push(isa::rrr(op::ALU, alu::ADD, 3, 1, 6)); // C = -1
    prog.push(isa::rrr(op::ALU, alu::ADC, 4, 0, 0)); // r4 = C (-1)
    prog.push(isa::rrr(op::ALUQ, alu::ADD, 5, 2, 7)); // C = +1
    prog.push(isa::rrr(op::ALU, alu::ADC, 7, 7, 0)); // r7 += C (+1)
    prog.push(isa::rrr(op::ALUQ, alu::ADD, 5, 2, 7)); // C = +1 again
    prog.push(isa::s6(op::BRQ, 0, cond::NOV, 2)); // C = 0? skip the count
    prog.push(isa::rrr(op::ALUQ, alu::ADD, 8, 8, 1));
    prog.push(isa::rrr(op::ALUQ, alu::SUB, 9, 9, 1));
    let here = prog.len() as i64;
    prog.push(isa::s6(op::BRQ, 0, cond::NE, top - here));
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    let st = both_ways(|_| {}, &prog, 2000);
    assert_eq!(reg_in(&st, 4), -1, "a negative carry");
    assert_eq!((reg_in(&st, 7), reg_in(&st, 8)), (40, 40), "positive carries, counted");
    assert_eq!(reg_in(&st, 3), w_max - 4, "the wrapped sum");
}

#[test]
fn a_misaligned_load_through_an_open_window_faults() {
    // a compiled loop of aligned loads opens the read window on the page;
    // then a load one tryte off a word centre must fault (vector -5)
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 2, 0, 1500)); // a word centre
    prog.extend(isa::wide(op::LI, 0, 4, 0, 1501)); // not one
    prog.push(isa::s6(op::LIQ, 0, 9, 5));
    let top = prog.len() as i64;
    prog.push(isa::rrr(op::MEM, mem::LDW, 1, 2, 0));
    prog.push(isa::rrr(op::ALUQ, alu::SUB, 9, 9, 1));
    let here = prog.len() as i64;
    prog.push(isa::s6(op::BRQ, 0, cond::NE, top - here));
    prog.push(isa::rrr(op::MEM, mem::LDW, 3, 4, 0)); // misaligned
    prog.push(isa::s6(op::LIQ, 0, 5, 77)); // never reached
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    for setup in [(|_: &mut Machine| {}) as fn(&mut Machine), paged] {
        let st = both_ways(setup, &prog, 60);
        assert_ne!(reg_in(&st, 5), 77, "the misaligned load faulted");
    }
}

#[test]
fn multiply_and_divide_in_a_compiled_loop() {
    // each round: r3 = r1 * r2 (large: overflows, C = sign of the high
    // half), r4 = MULH, r5 = r1 / r6, r7 = r1 % r6, r8 += C via ADC;
    // r1 and r2 grow and change sign; then a division by zero faults
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 1, 0, 123_456_789_012));
    prog.extend(isa::wide(op::LI, 0, 2, 0, -987_654_321));
    prog.extend(isa::wide(op::LI, 0, 6, 0, -97));
    prog.push(isa::s6(op::LIQ, 0, 9, 30));
    let top = prog.len() as i64;
    prog.push(isa::rrr(op::ALU, alu::MUL, 3, 1, 2));
    prog.push(isa::rrr(op::ALU, alu::ADC, 8, 8, 0));
    prog.push(isa::rrr(op::ALU, alu::MULH, 4, 1, 2));
    prog.push(isa::rrr(op::ALU, alu::DIV, 5, 1, 6));
    prog.push(isa::rrr(op::ALU, alu::MOD, 7, 1, 6));
    prog.push(isa::rrr(op::ALU, alu::ADD, 10, 10, 3)); // fold everything in
    prog.push(isa::rrr(op::ALU, alu::ADD, 10, 10, 4));
    prog.push(isa::rrr(op::ALU, alu::ADD, 10, 10, 5));
    prog.push(isa::rrr(op::ALU, alu::ADD, 10, 10, 7));
    prog.push(isa::rrr(op::ALUQ, alu::MUL, 1, 1, -7)); // grow, flip sign
    prog.push(isa::rrr(op::ALU, alu::SUB, 2, 2, 3));
    prog.push(isa::rrr(op::ALUQ, alu::SUB, 9, 9, 1));
    let here = prog.len() as i64;
    prog.push(isa::s6(op::BRQ, 0, cond::NE, top - here));
    prog.push(isa::rrr(op::ALU, alu::DIV, 11, 1, 0)); // by zero: faults
    prog.push(isa::s6(op::LIQ, 0, 12, 77)); // never reached
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    let st = both_ways(|_| {}, &prog, 1000);
    assert_ne!(reg_in(&st, 12), 77, "the division by zero faulted");
    assert_ne!(reg_in(&st, 8), 0, "products carried");
}

#[test]
fn a_load_one_page_up_misses_the_open_window() {
    // a compiled loop loads from page 1 (opening the read window there),
    // then from the same offset in page 2, which is mapped to frame 5, not
    // to the frame after page 1's
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 2, 0, PAGE + 300));
    prog.extend(isa::wide(op::LI, 0, 3, 0, 2 * PAGE + 300));
    prog.push(isa::s6(op::LIQ, 0, 9, 5));
    let top = prog.len() as i64;
    prog.push(isa::rrr(op::MEM, mem::LDW, 5, 2, 0));
    prog.push(isa::rrr(op::MEM, mem::LDW, 6, 3, 0));
    prog.extend(isa::wide(op::MEMD, mem::LDW, 7, 2, PAGE)); // page 2 again, by offset
    prog.push(isa::rrr(op::ALUQ, alu::SUB, 9, 9, 1));
    let here = prog.len() as i64;
    prog.push(isa::s6(op::BRQ, 0, cond::NE, top - here));
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    let setup = |m: &mut Machine| {
        paged(m);
        m.mem.write_word(4 * PAGE + 6, w(pte(5, 1, 1, -1))).unwrap(); // page 2 -> frame 5
        m.mem.write_word(PAGE + 300, w(111)).unwrap();
        m.mem.write_word(2 * PAGE + 300, w(222)).unwrap(); // frame 2: not mapped
        m.mem.write_word(5 * PAGE + 300, w(555)).unwrap();
    };
    let st = both_ways(setup, &prog, 60);
    assert_eq!((reg_in(&st, 5), reg_in(&st, 6), reg_in(&st, 7)), (111, 555, 555));
}

#[test]
fn an_address_that_wraps_round_is_wrapped_in_a_compiled_loop() {
    // base ≈ W_MAX + offset wraps to page −1 (mapped to frame 2); each
    // round loads, adds 1 and stores there, and steps 1500 words down,
    // so the fourth round's load reaches page −2, unmapped, and faults:
    // the fault address is the wrapped one
    let w_max = 3_812_798_742_493;
    let target = -PAGE + 300;
    let imm = target + 2 * w_max + 1 - w_max; // W_MAX + imm = target + 3^27
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 2, 0, w_max));
    prog.push(isa::s6(op::LIQ, 0, 9, 10));
    let top = prog.len() as i64;
    prog.extend(isa::wide(op::MEMD, mem::LDW, 5, 2, imm));
    prog.push(isa::rrr(op::ALUQ, alu::ADD, 5, 5, 1));
    prog.extend(isa::wide(op::MEMD, mem::STW, 5, 2, imm));
    prog.extend(isa::wide(op::ALUI, alu::SUB, 2, 2, 4500));
    prog.push(isa::rrr(op::ALUQ, alu::SUB, 9, 9, 1));
    let here = prog.len() as i64;
    prog.push(isa::s6(op::BRQ, 0, cond::NE, top - here));
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    let setup = |m: &mut Machine| {
        paged(m);
        m.mem.write_word(4 * PAGE - 3, w(pte(2, 1, 1, -1))).unwrap(); // page −1 -> frame 2
        for k in 0..3 {
            m.mem.write_word(2 * PAGE + 300 - 4500 * k, w(40 + k)).unwrap();
        }
    };
    let st = both_ways(setup, &prog, 60);
    assert_eq!(reg_in(&st, 9), 7, "three rounds ran");
    assert_eq!(reg_in(&st, 5), 43, "the third round's value, plus one");
    assert_eq!(st[5 * (cr::FADDR + 13) as usize + 1], target - 3 * 4500, "the fault address, wrapped");
}

#[test]
fn an_fpu_store_over_the_rest_of_its_block_runs_the_new_code() {
    // FLDD three instruction words, FSTD them over the next three: the
    // block interpreter must run the new ones (and so must compiled code)
    use setun_core::isa::fmem;
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 2, 0, 300)); // 0, 3
    prog.push(isa::rrr(op::FMEM, fmem::FLDD, 1, 2, 0)); // 6
    prog.extend(isa::wide(op::FMEMD, fmem::FSTD, 1, 0, 15)); // 9, 12: over 15, 18, 21
    prog.push(isa::s6(op::LIQ, 0, 5, 1)); // 15
    prog.push(isa::s6(op::LIQ, 0, 6, 1)); // 18
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0)); // 21
    let setup = |m: &mut Machine| {
        m.mem.load_words(300, &[isa::s6(op::LIQ, 0, 5, 2), isa::s6(op::LIQ, 0, 6, 2), isa::rrr(op::SYS, sys::HLT, 0, 0, 0)]);
    };
    for threshold in [u32::MAX, 1] {
        let (st, _) = both_ways_jit(threshold, setup, &prog, 10);
        assert_eq!((reg_in(&st, 5), reg_in(&st, 6)), (2, 2), "the stored instructions ran (threshold {threshold})");
    }
}

#[test]
fn an_fpu_store_across_into_a_read_only_page_writes_nothing() {
    // FSTD at the last two words of page 1 and the first of page 2, which
    // is read-only: a protection fault, and page 1 keeps its words
    use setun_core::isa::fmem;
    let at = PAGE + 9837; // word centres 29520, 29523 | 29526
    assert_eq!((at % 3, (at + 6 + 9841).div_euclid(PAGE), (at + 3 + 9841).div_euclid(PAGE)), (0, 2, 1));
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 2, 0, PAGE + 300));
    prog.push(isa::rrr(op::FMEM, fmem::FLDD, 1, 2, 0));
    prog.extend(isa::wide(op::LI, 0, 3, 0, at));
    prog.push(isa::rrr(op::FMEM, fmem::FSTD, 1, 3, 0));
    prog.push(isa::s6(op::LIQ, 0, 5, 77)); // never reached
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    let setup = move |m: &mut Machine| {
        paged(m);
        m.mem.write_word(4 * PAGE + 6, w(pte(2, -1, 1, -1))).unwrap(); // page 2: read-only
        m.mem.load_words(PAGE + 300, &[w(5), w(6), w(7)]);
        m.mem.load_words(at, &[w(111), w(222)]);
    };
    for threshold in [u32::MAX, 1] {
        let (st, fast) = both_ways_jit(threshold, setup, &prog, 12);
        assert_ne!(reg_in(&st, 5), 77, "the store faulted");
        assert_eq!((fast.mem.read_word(at).unwrap().value(), fast.mem.read_word(at + 3).unwrap().value()), (111, 222), "nothing written");
    }
}

#[test]
fn a_set_but_unmasked_fpu_flag_traps_every_time() {
    // FSR: IX already flagged but unmasked, the rest masked. 1/0 raises
    // only DZ (masked: no trap); then 1/2 is inexact (1/3 would be
    // exact: 0.1 in ternary) and must trap
    use setun_core::isa::{falu, fcvt};
    let p3 = |k: u32| 3i64.pow(k);
    let fsr = p3(13) + p3(12) + p3(11) + p3(10) + 1;
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 1, 0, fsr)); // 0, 3
    prog.push(isa::rrr(op::FCVT, fcvt::FMTS, 0, 1, 0)); // 6
    prog.push(isa::s6(op::LIQ, 0, 3, 1)); // 9
    prog.push(isa::rrr(op::FCVT, fcvt::FCVTIF, 1, 3, 0)); // 12
    prog.push(isa::rrr(op::FALU, falu::FDIV, 3, 1, 0)); // 15: 1/0
    prog.push(isa::s6(op::LIQ, 0, 3, 2)); // 18
    prog.push(isa::rrr(op::FCVT, fcvt::FCVTIF, 2, 3, 0)); // 21
    prog.push(isa::rrr(op::FALU, falu::FDIV, 4, 1, 2)); // 24: 1/2, traps
    prog.push(isa::s6(op::LIQ, 0, 5, 77)); // 27: never reached
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    for threshold in [u32::MAX, 1] {
        let (st, _) = both_ways_jit(threshold, |_| {}, &prog, 12);
        assert_ne!(reg_in(&st, 5), 77, "the inexact division trapped");
        assert_eq!(st[5 * (cr::EPC + 13) as usize + 1], 24, "at the second division, not the first");
    }
}


#[test]
fn tryte_loads_and_stores_through_the_windows_in_a_compiled_loop() {
    // nine consecutive trytes (every place in three words, extremes
    // included), each loaded, folded into an order-sensitive sum, and
    // stored plus one ten words further on; in frame 1, away from the
    // code (a store into the code's frame drops its compiled blocks)
    let trytes = [-9841i64, 0, 9841, 1, -1, 4321, -9000, 13, 9840];
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 2, 0, PAGE + 599)); // the first tryte of a word
    prog.push(isa::s6(op::LIQ, 0, 9, 9));
    let top = prog.len() as i64;
    prog.push(isa::rrr(op::MEM, mem::LDT, 5, 2, 0));
    prog.push(isa::rrr(op::ALU, alu::ADD, 10, 10, 5));
    prog.push(isa::rrr(op::ALUQ, alu::MUL, 11, 11, 3));
    prog.push(isa::rrr(op::ALU, alu::ADD, 11, 11, 5));
    prog.push(isa::rrr(op::ALUQ, alu::ADD, 6, 5, 1));
    prog.extend(isa::wide(op::MEMD, mem::STT, 6, 2, 30));
    prog.push(isa::rrr(op::ALUQ, alu::ADD, 2, 2, 1));
    prog.push(isa::rrr(op::ALUQ, alu::SUB, 9, 9, 1));
    let here = prog.len() as i64;
    prog.push(isa::s6(op::BRQ, 0, cond::NE, top - here));
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    let setup = move |m: &mut Machine| {
        for (k, &t) in trytes.iter().enumerate() {
            m.mem.write_tryte(PAGE + 599 + k as i64, ternary::Tryte::new(t).unwrap()).unwrap();
            // the store targets hold something to overwrite
            m.mem.write_tryte(PAGE + 629 + k as i64, ternary::Tryte::new(7 - k as i64 * 1000).unwrap()).unwrap();
        }
    };
    let (st, fast) = both_ways_jit(1, setup, &prog, 200);
    assert!(fast.cpu.jit_stats()[2] > 50, "the loop ran compiled: {:?}", fast.cpu.jit_stats());
    assert_eq!(reg_in(&st, 10), trytes.iter().sum::<i64>());
    for (k, &t) in trytes.iter().enumerate() {
        let want = if t == 9841 { -9841 } else { t + 1 }; // the low tryte of t + 1
        assert_eq!(fast.mem.read_tryte(PAGE + 629 + k as i64).unwrap().value(), want, "tryte {k}");
    }
}

#[test]
fn a_tryte_store_into_code_ahead_runs_the_new_code() {
    // three rounds; the STT goes to 31 + (r9 - 1) * PAGE: data in frames 2
    // and 1 (the loop's block is built and compiled meanwhile), then the
    // operand tryte of the LIQ r5, 0 further on, which must then give 7
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 1, 0, 5 * 729 + 7)); // 0, 3: LIQ r5, 7's operand tryte
    prog.push(isa::s6(op::LIQ, 0, 9, 3)); // 6
    prog.push(isa::rrr(op::ALUQ, alu::SUB, 2, 9, 1)); // 9
    prog.extend(isa::wide(op::ALUI, alu::MUL, 2, 2, PAGE)); // 12, 15
    prog.extend(isa::wide(op::ALUI, alu::ADD, 2, 2, 31)); // 18, 21
    prog.push(isa::rrr(op::MEM, mem::STT, 1, 2, 0)); // 24
    prog.push(isa::rrr(op::ALUQ, alu::SUB, 9, 9, 1)); // 27
    prog.push(isa::s6(op::LIQ, 0, 5, 0)); // 30: its low tryte is at 31
    prog.push(isa::s6(op::BRQ, 0, cond::NE, -8)); // 33: to 9
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0)); // 36
    for setup in [(|_: &mut Machine| {}) as fn(&mut Machine), paged] {
        for threshold in [u32::MAX, 1] {
            let (st, fast) = both_ways_jit(threshold, setup, &prog, 40);
            assert_eq!(reg_in(&st, 5), 7, "the rewritten instruction ran (threshold {threshold})");
            if threshold == 1 {
                assert!(fast.cpu.jit_stats()[2] > 0, "the last round ran compiled");
            }
        }
    }
}

#[test]
fn a_tryte_load_one_past_its_page_misses_the_window() {
    // the last tryte of page 1, then the first of page 2, which is mapped
    // to frame 5 (frame 2, next in host memory, is not mapped)
    let last = PAGE + 9841;
    let mut prog = Vec::new();
    prog.extend(isa::wide(op::LI, 0, 2, 0, last));
    prog.push(isa::s6(op::LIQ, 0, 9, 4));
    let top = prog.len() as i64;
    prog.push(isa::rrr(op::MEM, mem::LDT, 5, 2, 0));
    prog.push(isa::rrr(op::MEM, mem::LDT, 6, 2, 1));
    prog.push(isa::rrr(op::ALUQ, alu::SUB, 9, 9, 1));
    let here = prog.len() as i64;
    prog.push(isa::s6(op::BRQ, 0, cond::NE, top - here));
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    let setup = move |m: &mut Machine| {
        paged(m);
        m.mem.write_word(4 * PAGE + 6, w(pte(5, 1, 1, -1))).unwrap(); // page 2 -> frame 5
        m.mem.write_tryte(last, ternary::Tryte::new(111).unwrap()).unwrap();
        m.mem.write_tryte(2 * PAGE - 9841, ternary::Tryte::new(222).unwrap()).unwrap();
        m.mem.write_tryte(5 * PAGE - 9841, ternary::Tryte::new(555).unwrap()).unwrap();
    };
    let (st, _) = both_ways_jit(1, setup, &prog, 40);
    assert_eq!((reg_in(&st, 5), reg_in(&st, 6)), (111, 555));
}

#[test]
fn compiled_regions_entered_at_any_block_agree() {
    // a loop of several blocks (a branch inside), run in odd-sized slices
    // so batches end, and compiled code is re-entered, at every block
    let mut prog = Vec::new();
    prog.push(isa::s6(op::LIQ, 0, 9, 200)); // 0
    let top = prog.len() as i64; // 3
    prog.push(isa::rrr(op::ALUQ, alu::ADD, 1, 1, 3)); // 3
    prog.push(isa::rrr(op::ALUQ, alu::MOD, 2, 1, 7)); // 6
    prog.push(isa::rrr(op::ALUQ, alu::CMP, 0, 2, 3)); // 9
    prog.push(isa::s6(op::BRQ, 0, cond::GE, 3)); // 12: to 21
    prog.push(isa::rrr(op::ALU, alu::ADD, 3, 3, 2)); // 15
    prog.push(isa::s6(op::BRQ, 0, cond::ALWAYS, 2)); // 18: to 24
    prog.push(isa::rrr(op::ALU, alu::SUB, 4, 4, 2)); // 21
    prog.push(isa::rrr(op::ALUQ, alu::MUL, 5, 3, 2)); // 24
    prog.push(isa::rrr(op::ALU, alu::ADD, 5, 5, 4)); // 27
    prog.push(isa::rrr(op::ALUQ, alu::SUB, 9, 9, 1)); // 30
    let here = prog.len() as i64;
    prog.push(isa::s6(op::BRQ, 0, cond::NE, top - here)); // 33
    prog.push(isa::rrr(op::SYS, sys::HLT, 0, 0, 0));
    // (and the least that must run compiled: entering only at a region's
    // root gives about 1100, 1280, 1400 and 1850)
    for (slice, least) in [(7u64, 1300), (11, 1450), (13, 1500), (1000, 1800)] {
        let mut fast = Machine::new(FRAMES, 0);
        let mut slow = Machine::new(FRAMES, 0);
        for m in [&mut fast, &mut slow] {
            m.mem.load_words(0, &prog);
            m.cpu.pc = Word::ZERO;
        }
        // not eager (threshold 1 makes every block a region of its own):
        // one region covers the loop
        fast.cpu.set_jit_threshold(3);
        fast.cpu.set_jit_wait(true);
        let steps = 3000;
        let mut done = 0;
        while done < steps {
            fast.run(slice.min(steps - done));
            done += slice.min(steps - done);
        }
        for _ in 0..steps {
            slow.step();
        }
        assert_eq!(digest(&mut fast), digest(&mut slow), "slices of {slice}");
        let st = fast.cpu.jit_stats();
        assert!(st[2] > least, "compiled code entered mid-loop (slices of {slice}): {st:?}");
    }
}
