//! Core 1 launch: starts the C MS/TP master state machine.
//!
//! Core 1 runs the timing-critical bacnet-stack MS/TP master loop (C code in
//! `csrc/core1_entry.c`). We allocate a static stack for it here and launch
//! it with `embassy_rp::multicore::spawn_core1`.

use embassy_rp::multicore::{spawn_core1, Stack};
use embassy_rp::peripherals::CORE1;

// ---------------------------------------------------------------------------
// Watchdog heartbeat (C2)
// ---------------------------------------------------------------------------

/// Heartbeat counter incremented by Core 1 on every iteration of its main loop.
///
/// Core 0 should read this periodically (e.g. every 200 ms) and compare
/// against the previously observed value.  If the value has not changed for
/// more than one sample interval, Core 1 has stalled and the RP2040 hardware
/// watchdog should be triggered to reset both cores.
///
/// Declared `#[no_mangle]` so the C symbol `core1_heartbeat` resolves here.
///
/// # TODO
/// - Check `core1_heartbeat` periodically in the Core 0 supervisor task;
///   if stale for > 200 ms, trigger watchdog reset via RP2040 WATCHDOG_CTRL.
/// - Enable the RP2040 hardware watchdog in a future phase using
///   `embassy_rp::watchdog::Watchdog` with a 500 ms window; Core 0 feeds
///   the watchdog only when Core 1's heartbeat is live.
#[no_mangle]
pub static mut core1_heartbeat: u32 = 0;

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
