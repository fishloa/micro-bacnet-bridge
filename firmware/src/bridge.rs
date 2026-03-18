//! Bridge state: shared tables of discovered BACnet devices and their points.
//!
//! The platform-independent data types and logic live in `bridge_core::bridge`.
//! This module wraps `BridgeStateInner` in an embassy
//! `CriticalSectionRawMutex`-guarded `Mutex` so it can be shared safely
//! across async tasks running on Core 0.

use bridge_core::bridge::BridgeStateInner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;

// Re-export the core types so the rest of the firmware can import them from
// `crate::bridge::*` without depending on bridge_core directly.
#[allow(unused_imports)]
pub use bridge_core::bridge::{DeviceEntry, PointEntry, MAX_DEVICES, MAX_POINTS_PER_DEVICE};

// Keep the old MAX_POINTS alias for any code still using it.
#[allow(dead_code)]
pub const MAX_POINTS: usize = MAX_POINTS_PER_DEVICE;

/// Global bridge state, accessible from any async task via the mutex.
pub static BRIDGE_STATE: Mutex<CriticalSectionRawMutex, BridgeStateInner> =
    Mutex::new(BridgeStateInner::new());

/// Helper: return device count without holding the lock long.
pub async fn device_count() -> usize {
    BRIDGE_STATE.lock().await.device_count
}

/// Helper: copy out device list.  Returns number of entries copied.
pub async fn snapshot_devices(out: &mut [DeviceEntry]) -> usize {
    let state = BRIDGE_STATE.lock().await;
    let n = state.device_count.min(out.len());
    out[..n].clone_from_slice(&state.devices[..n]);
    n
}
