//! FFI bindings to the C bacnet-stack library compiled for Core 1.
//!
//! The C code lives in `csrc/` and is compiled by `build.rs` into `libbacnet.a`.
//! All symbols are accessed via the `extern "C"` block below.
//!
//! The IPC ring buffers (`MSTP_TO_IP_RING`, `IP_TO_MSTP_RING`) are declared in
//! `ipc.rs` as `#[no_mangle]` statics; the C code references them as `extern`
//! symbols without any explicit declaration here.

// C-callable bacnet-stack API (Core 1 / MS/TP master functions).
// These are called from `core1_entry.c` and declared here for documentation;
// the linker resolves them from `libbacnet.a`.
#[allow(dead_code)]
extern "C" {
    // Initialise the MS/TP master with the given parameters.
    // Must be called once from Core 1 before entering the main loop.
    pub fn mstp_master_init(mac: u8, baud: u32, max_master: u8);

    // Process one iteration of the MS/TP token-passing state machine.
    // Called in a tight loop on Core 1.
    pub fn mstp_master_tick();

    // Send a BACnet PDU to the MS/TP bus.
    // `data` must remain valid until the function returns.
    pub fn mstp_send_pdu(dest_mac: u8, data: *const u8, data_len: u16);
}
