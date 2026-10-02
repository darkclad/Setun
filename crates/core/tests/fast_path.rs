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
    for seed in 1..=300u64 {
        let mut fast = machine(seed);
        let mut slow = machine(seed);
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
    }
    assert!(steps_total > 100_000, "the programs ran ({steps_total} steps)");
}

// ---- directed: the caches must notice changes right away ------------------

use setun_core::isa::{self, alu, cond, mem, sys};

/// Load `prog` at 0 and run it both ways for `steps`; the states must match.
fn both_ways(setup: impl Fn(&mut Machine), prog: &[Word], steps: u64) -> Vec<i64> {
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
    fast.run(steps);
    for _ in 0..steps {
        slow.step();
    }
    let (a, b) = (digest(&mut fast), digest(&mut slow));
    assert_eq!(a, b, "fast and reference paths differ");
    a
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
