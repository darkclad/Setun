//! Print how much of the kernel's code and data areas is used.
fn main() {
    let dir = trinux::kernel_dir();
    let kb = trine::build_file(&dir.join("main.tri"), &trinux::kernel_options()).unwrap();
    let page = setun_core::PAGE as usize;
    println!(
        "kernel code: {} trytes = {:.2} of {} sectors",
        kb.image.len(),
        kb.image.len() as f64 / page as f64,
        trinux::KERNEL_MAX_SECTORS
    );
    println!(
        "kernel data: {} .. {} = {} trytes, {:.1}% of frames 31..57",
        kb.data_start,
        kb.data_end,
        kb.data_end - kb.data_start,
        100.0 * (kb.data_end - trinux::KERNEL_DATA) as f64 / (trinux::KERNEL_DATA_END - trinux::KERNEL_DATA) as f64
    );
}
