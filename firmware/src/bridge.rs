//! Bridge state: shared tables of discovered BACnet devices and their points.
//!
//! The platform-independent data types and logic live in `bridge_core::bridge`.
//! This module wraps `BridgeStateInner` in an embassy
//! `CriticalSectionRawMutex`-guarded `Mutex` so it can be shared safely
//! across async tasks running on Core 0.

use bridge_core::bridge::BridgeStateInner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;

/// Global bridge state, accessible from any async task via the mutex.
pub static BRIDGE_STATE: Mutex<CriticalSectionRawMutex, BridgeStateInner> =
    Mutex::new(BridgeStateInner::new());
