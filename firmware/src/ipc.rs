//! Rust-side IPC: static ring buffer instances shared with Core 1 (C side).
//!
//! Two SPSC ring buffers carry BACnet PDUs between the two cores:
//! - `MSTP_TO_IP`: Core 1 (MS/TP master) → Core 0 (BACnet/IP)
//! - `IP_TO_MSTP`: Core 0 (BACnet/IP)    → Core 1 (MS/TP master)
//!
//! The buffers are declared as `#[no_mangle]` static variables so the C code
//! on Core 1 can reference them as `extern` symbols without any Rust glue.

use bridge_core::ipc::RingBuffer;

/// PDUs flowing from the MS/TP side (Core 1) to the BACnet/IP side (Core 0).
#[no_mangle]
pub static mut MSTP_TO_IP_RING: RingBuffer<8> = RingBuffer::new();

/// PDUs flowing from the BACnet/IP side (Core 0) to the MS/TP side (Core 1).
#[no_mangle]
pub static mut IP_TO_MSTP_RING: RingBuffer<8> = RingBuffer::new();

/// Return a mutable reference to the MS/TP → IP ring buffer.
///
/// # Safety
/// The caller must ensure that only one side (Core 0 consumer) holds this
/// reference at a time. The SPSC contract must be maintained.
#[inline]
pub fn mstp_to_ip() -> &'static mut RingBuffer<8> {
    // SAFETY: SPSC contract: Core 1 is the sole producer, Core 0 the sole consumer.
    // We use raw pointer + deref to avoid the static_mut_refs lint.
    unsafe { &mut *core::ptr::addr_of_mut!(MSTP_TO_IP_RING) }
}

/// Return a mutable reference to the IP → MS/TP ring buffer.
///
/// # Safety
/// The caller must ensure that only one side (Core 0 producer) holds this
/// reference at a time.
#[inline]
pub fn ip_to_mstp() -> &'static mut RingBuffer<8> {
    // SAFETY: SPSC contract: Core 0 is the sole producer, Core 1 the sole consumer.
    unsafe { &mut *core::ptr::addr_of_mut!(IP_TO_MSTP_RING) }
}
