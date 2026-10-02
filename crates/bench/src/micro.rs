//! Microbenchmarks: tight guest loops of one instruction kind, to see what
//! a single guest instruction costs on the host, part by part.

use setun_core::Machine;
use std::time::Instant;

/// Each body is repeated 27 times inside a counted loop.
const CASES: &[(&str, &str)] = &[
    ("add r,r", "add r1, r1, r2"),
    ("add r,k", "add r1, r1, 5"),
    ("cmp", "cmp r1, r2"),
    ("li", "li r3, 100"),
    ("mul", "mul r1, r2, r2"),
    ("ldw", "ldw r1, [sp+3]"),
    ("stw", "stw r1, [sp+3]"),
    ("ldt", "ldt r1, [sp+1]"),
    ("push+pop", "stw r1, [sp-3]\n sub sp, sp, 3\n ldw r1, [sp]\n add sp, sp, 3"),
    ("b (taken)", "b $+3"),
    ("bne untaken", "beq $+3"),
    ("nop (generic)", "nop"),
];

fn program(body: &str) -> String {
    let mut s = String::from(
        "        .org -59049\n        li sp, 6000\n        li r2, 7\n        li r9, 400000\nloop:\n",
    );
    for _ in 0..27 {
        s.push_str("        ");
        s.push_str(body);
        s.push('\n');
    }
    s.push_str("        sub r9, r9, 1\n        bne loop\n        hlt\n");
    s
}

fn rdtsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    0
}

pub fn run() {
    println!("{:<14} {:>12} {:>9} {:>10}", "case", "instructions", "ns/insn", "tsc/insn");
    for (name, body) in CASES {
        let p = setun_asm::text::assemble(&program(body)).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let rom = setun_asm::rom_image(&p.segments).expect("fits in ROM");
        let mut m = Machine::new(27, 0);
        m.load_rom(&rom);
        let t = Instant::now();
        let c0 = rdtsc();
        while !m.cpu.halted {
            m.run(10_000_000);
        }
        let cycles = rdtsc() - c0;
        let secs = t.elapsed().as_secs_f64();
        let n = m.cpu.cr(setun_core::isa::cr::CYCLE).value() as f64;
        println!("{:<14} {:>12} {:>9.2} {:>10.1}", name, n as u64, secs * 1e9 / n, cycles as f64 / n);
    }
}
