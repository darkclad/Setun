//! A guest profiler for ROM programs: sample the PC every few steps and
//! charge it to the Trine function it is in.

use setun_core::Machine;
use std::path::Path;

pub fn run(path: &Path) {
    let b = trine::build_file(path, &trine::Options::rom()).unwrap_or_else(|e| panic!("{e}"));
    // functions: symbols that are not local labels or runtime internals
    let mut funcs: Vec<(i64, &str)> = b
        .program
        .symbols
        .iter()
        .filter(|(n, _)| !n.starts_with('.') && !n.starts_with("_L"))
        .map(|(n, &a)| (a, n.as_str()))
        .collect();
    funcs.sort();
    let mut m = Machine::new(729, 1_790_000_000);
    m.load_rom(&b.image);
    let mut hits: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
    let mut total = 0u64;
    let mut steps = 0u64;
    while !m.powered_off && !m.cpu.halted && steps < 2_000_000_000 {
        m.run(97);
        steps += 97;
        let pc = m.cpu.pc.value();
        let k = funcs.partition_point(|f| f.0 <= pc);
        let name = if k == 0 { "?" } else { funcs[k - 1].1 };
        *hits.entry(name).or_default() += 1;
        total += 1;
    }
    let mut v: Vec<_> = hits.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    println!("{} steps, {total} samples", steps);
    for (n, c) in v.iter().take(20) {
        println!("{:>6.2}%  {n}", 100.0 * *c as f64 / total as f64);
    }
}
