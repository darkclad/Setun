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

/// The Trinux workload profiled: each sample is charged to a kernel
/// function, or to a function of the user program whose code is at the PC
/// (found by matching the code there against each program's image).
pub fn trinux(script: &str) {
    use setun_core::cpu::Access;
    type Funcs = Vec<(i64, String)>;
    let funcs_of = |p: &setun_asm::text::Program| -> Funcs {
        let mut v: Funcs = p
            .symbols
            .iter()
            .filter(|(n, _)| !n.starts_with('.') && !n.starts_with("_L"))
            .map(|(n, &a)| (a, n.clone()))
            .collect();
        v.sort();
        v
    };
    let lookup = |f: &Funcs, pc: i64| -> String {
        let k = f.partition_point(|x| x.0 <= pc);
        if k == 0 { "?".into() } else { f[k - 1].1.clone() }
    };
    let kdir = trinux::kernel_dir();
    let kb = trine::build_file(&kdir.join("main.tri"), &trinux::kernel_options()).unwrap_or_else(|e| panic!("{e}"));
    let kfuncs = funcs_of(&kb.program);
    let kend = trinux::KERNEL_ORG + kb.image.len() as i64;
    // the user programs, one build per source
    let mut bins: Vec<(String, Vec<i16>, Funcs)> = Vec::new();
    for (name, src) in trinux::default_files() {
        if src.extension().is_none_or(|e| e != "tri") || bins.iter().any(|b| b.0.ends_with(&*src.file_stem().unwrap().to_string_lossy())) {
            continue;
        }
        let _ = name;
        let b = trine::build_file(&src, &trine::Options::user()).unwrap_or_else(|e| panic!("{e}"));
        bins.push((src.file_stem().unwrap().to_string_lossy().into_owned(), b.image, funcs_of(&b.program)));
    }

    let disk = trinux::build_disk(&kdir).expect("disk builds");
    let mut m = Machine::new(729, 1_790_000_000);
    m.load_rom(&trine::bios_rom().expect("BIOS builds"));
    m.dev.disks[0].attach(Box::new(setun_core::devices::MemDisk::from_trytes(&disk.trytes)));
    let mut serial = String::new();
    let mut hits: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let mut per_bin: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let mut total = 0u64;
    let mut sent = false;
    let mut prompts = 0;
    loop {
        m.run(97);
        let out = m.dev.serial.take_output();
        serial.push_str(&out);
        if serial.ends_with("login: ") || serial.ends_with("Password: ") {
            m.dev.serial_input("root\n");
            serial.push('\n');
        } else if serial.ends_with("$ ") {
            serial.push('\n');
            prompts += 1;
            if sent {
                break;
            }
            m.dev.serial_input(script);
            sent = true;
        }
        if !sent || prompts == 0 {
            continue; // profile the script only
        }
        let pc = m.cpu.pc.value();
        let (bin, func) = if (trinux::KERNEL_ORG..kend).contains(&pc) {
            ("kernel".to_string(), lookup(&kfuncs, pc))
        } else {
            let code: Option<Vec<i16>> = m
                .cpu
                .translate(&mut m.mem, pc, Access::Fetch)
                .ok()
                .and_then(|pa| (-1..5).map(|d| m.mem.read_tryte(pa + d).ok().map(|t| t.value() as i16)).collect());
            let i = (pc - trine::USER_BASE) as usize;
            match code.and_then(|c| bins.iter().find(|b| b.1.get(i..i + 6) == Some(&c[..]))) {
                Some(b) => (b.0.clone(), lookup(&b.2, pc)),
                None => ("?".into(), "?".into()),
            }
        };
        *per_bin.entry(bin.clone()).or_default() += 1;
        *hits.entry(format!("{bin}:{func}")).or_default() += 1;
        total += 1;
    }
    let show = |h: std::collections::HashMap<String, u64>, n: usize| {
        let mut v: Vec<_> = h.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        for (k, c) in v.iter().take(n) {
            println!("{:>6.2}%  {k}", 100.0 * *c as f64 / total as f64);
        }
    };
    println!("{total} samples (every 97 steps)");
    show(per_bin, 12);
    println!();
    show(hits, 40);
}
