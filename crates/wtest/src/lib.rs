//! The fast-path tests in WebAssembly, for the browser build's JIT:
//! `target/wtest.mjs` runs each in a fresh instance under node, compiling
//! the JIT's modules synchronously when asked (`setun_jit_service`).
#![cfg(target_arch = "wasm32")]

#[allow(dead_code, clippy::all)]
mod t {
    include!(concat!(env!("OUT_DIR"), "/fast_path_tests.rs"));
}

use std::cell::RefCell;

thread_local! {
    static MSG: RefCell<String> = const { RefCell::new(String::new()) };
    static OUT: RefCell<(u64, Vec<u8>)> = const { RefCell::new((0, Vec::new())) };
}

#[no_mangle]
pub extern "C" fn wtest_count() -> u32 {
    t::TESTS.len() as u32
}

#[no_mangle]
pub extern "C" fn wtest_name_ptr(i: u32) -> *const u8 {
    t::TESTS[i as usize].0.as_ptr()
}

#[no_mangle]
pub extern "C" fn wtest_name_len(i: u32) -> u32 {
    t::TESTS[i as usize].0.len() as u32
}

/// Run test `i`. A failure panics, which traps: the host reads the message
/// (`wtest_msg_*`) and starts a fresh instance for the next test.
#[no_mangle]
pub extern "C" fn wtest_run(i: u32) {
    std::panic::set_hook(Box::new(|info| MSG.with(|m| *m.borrow_mut() = info.to_string())));
    (t::TESTS[i as usize].1)();
}

#[no_mangle]
pub extern "C" fn wtest_msg_ptr() -> *const u8 {
    MSG.with(|m| m.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn wtest_msg_len() -> u32 {
    MSG.with(|m| m.borrow().len() as u32)
}

/// The next JIT module (its length, 0: none), its bytes and id.
#[no_mangle]
pub extern "C" fn wtest_jit_take() -> u32 {
    match setun_core::cpu::Cpu::jit_take() {
        Some(r) => OUT.with(|o| {
            *o.borrow_mut() = r;
            o.borrow().1.len() as u32
        }),
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn wtest_jit_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().1.as_ptr())
}

#[no_mangle]
pub extern "C" fn wtest_jit_id() -> f64 {
    OUT.with(|o| o.borrow().0 as f64)
}

#[no_mangle]
pub extern "C" fn wtest_jit_done(id: f64, index: u32) {
    setun_core::cpu::Cpu::jit_finished(id as u64, index);
}
