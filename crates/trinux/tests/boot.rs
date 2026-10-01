//! K1: BIOS → loader → start_kernel on the emulator.

use setun_core::isa::{cr, mode};

#[test]
fn kernel_boots_with_paging_and_timer() {
    let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| panic!("{e}"));
    assert!(disk.kernel_sectors >= 1);
    let mut m = trinux::boot(&disk, 729, 60_000_000);
    let out = m.dev.serial.take_output();
    for line in [
        "Trinux loader: kernel",
        "Trinux version 0.1 (trine) on Setun-386, balanced ternary",
        "Booted from disk 0, RAM 729 pages",
        "Traps: 243 vectors",
        "Memory: 689 pages free (frames 40..728), triddy orders 0..9",
        "mm: triddy self-test OK",
        "Paging: 9|9|9, kernel identity-mapped in L1 slots 0..+1, 759 pages",
        "Timer: 100 Hz",
        "K1 OK: 10 timer ticks, idle",
    ] {
        assert!(out.contains(line), "missing {line:?} in:\n{out}");
    }
    assert!(!out.contains("panic"), "{out}");
    // printk lines carry Linux-style uptime stamps.
    assert!(out.contains("[    0."), "{out}");
    // The CPU really is in paged kernel mode, idling.
    assert_eq!(m.cpu.mode(mode::PG), 1);
    assert_eq!(m.cpu.ring(), 1);
    assert_eq!(m.cpu.mode(mode::IE), 1);
    assert!(m.cpu.cr(cr::PTB).value() % 59_049 == 0, "PTB is a multiple of 3^10");
}

#[test]
fn kernel_fault_panics_with_details() {
    // A kernel that dereferences a misaligned pointer after traps_init.
    let dir = std::env::temp_dir().join(format!("trinux-panic-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("boot")).unwrap();
    for f in ["params.tri", "printk.tri", "mm.tri", "arch.tri", "boot/loader.tri"] {
        std::fs::copy(trinux::kernel_dir().join(f), dir.join(f)).unwrap();
    }
    std::fs::write(
        dir.join("main.tri"),
        "import \"arch.tri\";\nfn main(d: int, f: int) -> int { traps_init(); var p: *int = 1 as *int; return *p; }\n",
    )
    .unwrap();
    let disk = trinux::build_disk(&dir).unwrap_or_else(|e| panic!("{e}"));
    let mut m = trinux::boot(&disk, 729, 20_000_000);
    let out = m.dev.serial.take_output();
    std::fs::remove_dir_all(&dir).ok();
    assert!(out.contains("trap: misaligned access (vector -5)"), "{out}");
    assert!(out.contains("address 1"), "{out}");
    assert!(out.contains("Kernel panic: unhandled trap in the kernel"), "{out}");
}
