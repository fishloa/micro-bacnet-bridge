//! Core 1 launch: starts the C MS/TP master state machine.
//!
//! Core 1 runs the timing-critical bacnet-stack MS/TP master loop (C code in
//! `csrc/core1_entry.c`). We allocate a static stack for it here and launch
//! it with `embassy_rp::multicore::spawn_core1`.

use embassy_rp::multicore::{spawn_core1, Stack};
use embassy_rp::peripherals::CORE1;

/// Stack allocated for Core 1. 8 KB is sufficient for the C MS/TP state machine.
static mut CORE1_STACK: Stack<8192> = Stack::new();

extern "C" {
    /// Entry point implemented in `csrc/core1_entry.c`.
    /// Called from Core 1; never returns.
    fn core1_entry() -> !;
}

/// Launch Core 1, which runs the C MS/TP master state machine.
///
/// Must be called exactly once during startup, before any IPC ring buffer
/// access from Core 1.
pub fn launch_core1(core1: embassy_rp::Peri<'static, CORE1>) {
    // SAFETY: CORE1_STACK is only written here (once, before Core 1 runs).
    let stack = unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) };
    spawn_core1(core1, stack, move || {
        // SAFETY: core1_entry() is implemented in C and never returns.
        unsafe { core1_entry() }
    });
}
