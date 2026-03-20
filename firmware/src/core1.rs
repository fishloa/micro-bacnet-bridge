//! Core 1 launch: starts the C MS/TP master state machine.
//!
//! Core 1 runs the timing-critical bacnet-stack MS/TP master loop (C code in
//! `csrc/core1_entry.c`). We allocate a static stack for it here and launch
//! it with `embassy_rp::multicore::spawn_core1`.

use embassy_rp::multicore::{spawn_core1, Stack};
use embassy_rp::peripherals::CORE1;
use portable_atomic::AtomicU32;

// ---------------------------------------------------------------------------
// Watchdog heartbeat (C2)
// ---------------------------------------------------------------------------

/// Heartbeat counter incremented by Core 1 on every iteration of its main loop.
///
/// Core 0 should read this periodically (e.g. every 200 ms) and compare
/// against the previously observed value.  If the value has not changed for
/// more than one sample interval, Core 1 has stalled and the RP2350A hardware
/// watchdog should be triggered to reset both cores.
///
/// Declared `#[no_mangle]` so the C symbol `core1_heartbeat` resolves here.
///
/// `AtomicU32` has `repr(transparent)` layout (same as `u32`), so the C side
/// accessing it as `extern volatile uint32_t core1_heartbeat` sees the same
/// memory.  Core 1 is the only writer (via `core1_heartbeat++`), which is a
/// non-atomic load-add-store on Cortex-M33, but since there is exactly one
/// writer and one reader, a torn read is harmless — Core 0 merely sees either
/// the old or new value.  Relaxed ordering is therefore correct.
///
/// # TODO
/// - Check `core1_heartbeat` periodically in the Core 0 supervisor task;
///   if stale for > 200 ms, trigger watchdog reset via RP2350A WATCHDOG_CTRL.
/// - Enable the RP2350A hardware watchdog in a future phase using
///   `embassy_rp::watchdog::Watchdog` with a 500 ms window; Core 0 feeds
///   the watchdog only when Core 1's heartbeat is live.
#[no_mangle]
pub static core1_heartbeat: AtomicU32 = AtomicU32::new(0);

/// Stack allocated for Core 1. 8 KB is sufficient for the C MS/TP state machine.
static mut CORE1_STACK: Stack<8192> = Stack::new();

/// MS/TP config struct matching `mstp_config_t` in `bacnet_bridge.h`.
#[repr(C)]
struct MstpConfig {
    baud_rate: u32,
    mac_address: u8,
    max_master: u8,
    _pad: [u8; 2],
}

/// MS/TP status struct matching `mstp_status_t` in `bacnet_bridge.h`.
#[repr(C)]
pub struct MstpStatus {
    pub active_baud: u32,
    pub frames_rx: u32,
    pub frames_tx: u32,
    pub errors_rx: u32,
    pub bus_active: u8,
    pub detecting: u8,
    pub parity: u8,
    _pad: u8,
}

extern "C" {
    /// Entry point implemented in `csrc/core1_entry.c`.
    /// Called from Core 1; never returns.
    fn core1_entry() -> !;

    /// Shared config struct read by Core 1 at startup.
    static mut g_mstp_config: MstpConfig;

    /// Shared status struct written by Core 1.
    static g_mstp_status: MstpStatus;

    /// Flash pause handshake flags.
    #[link_name = "g_flash_pause_request"]
    pub static mut g_flash_pause_request_raw: u8;
    static g_core1_paused: u8;
}

/// Pause Core 1 for flash operations.
///
/// Sets the pause flag and waits for Core 1 to acknowledge it is spinning
/// in SRAM with the SIO FIFO interrupt disabled. This prevents embassy-rp's
/// `in_ram()` from triggering the flash-resident SIO_IRQ_FIFO ISR on Core 1.
///
/// Returns a guard that resumes Core 1 on drop.
///
/// Used by `config::ConfigManager::save` (currently `#[allow(dead_code)]`) for
/// config-sector writes. Retained here for when the web-UI config save path is
/// wired up.
#[allow(dead_code)]
pub fn pause_core1_for_flash() -> FlashPauseGuard {
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(g_flash_pause_request_raw), 1);
        // Core 1 may be in auto-detect (up to 2s per baud rate × 4 rates = 8s).
        // At ~150MHz, 150M iterations ≈ 1 second. Wait up to 10 seconds.
        let mut timeout = 1_500_000_000u32;
        while core::ptr::read_volatile(core::ptr::addr_of!(g_core1_paused)) == 0 && timeout > 0 {
            timeout -= 1;
            cortex_m::asm::nop();
        }
    }
    FlashPauseGuard(())
}

/// RAII guard that resumes Core 1 when dropped.
#[allow(dead_code)]
pub struct FlashPauseGuard(());

impl Drop for FlashPauseGuard {
    fn drop(&mut self) {
        unsafe {
            core::ptr::write_volatile(core::ptr::addr_of_mut!(g_flash_pause_request_raw), 0);
        }
    }
}

/// Read the current MS/TP serial port status from Core 1.
pub fn mstp_status() -> (u32, u32, u32, u32, bool, bool) {
    // SAFETY: g_mstp_status is written only by Core 1, read-only from Core 0.
    // Individual u32 reads are atomic on Cortex-M33.
    unsafe {
        let s = &*core::ptr::addr_of!(g_mstp_status);
        (
            s.active_baud,
            s.frames_rx,
            s.frames_tx,
            s.errors_rx,
            s.bus_active != 0,
            s.detecting != 0,
        )
    }
}

/// Launch Core 1, which runs the C MS/TP master state machine.
///
/// Writes the MS/TP configuration to `g_mstp_config` before launching Core 1,
/// so the C code can read the baud rate, MAC address, and max master values.
///
/// Must be called exactly once during startup, before any IPC ring buffer
/// access from Core 1.
pub fn launch_core1(core1: embassy_rp::Peri<'static, CORE1>, baud: u32, mac: u8, max_master: u8) {
    // Write config before Core 1 starts.
    unsafe {
        let cfg = &mut *core::ptr::addr_of_mut!(g_mstp_config);
        cfg.baud_rate = baud;
        cfg.mac_address = mac;
        cfg.max_master = max_master;
    }

    let stack = unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) };
    spawn_core1(core1, stack, move || unsafe { core1_entry() });
}
