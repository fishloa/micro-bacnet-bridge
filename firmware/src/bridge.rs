//! Bridge state: shared tables of discovered BACnet devices and their points.
//!
//! `BridgeState` is protected by an `embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex`
//! so it can be read from both the HTTP task and the BACnet/IP task.

use bridge_core::bacnet::{BacnetValue, ObjectId};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use heapless::String;

/// Maximum number of BACnet devices we track simultaneously.
pub const MAX_DEVICES: usize = 8;

/// Maximum number of points per device.
pub const MAX_POINTS: usize = 32;

/// A discovered BACnet device entry.
#[derive(Clone)]
pub struct DeviceEntry {
    /// BACnet device instance number.
    pub device_id: u32,
    /// Device name from Object_Name property (or empty if not yet read).
    pub name: String<32>,
    /// Source IP:port as 4+2 bytes (BACnet/IP address); [0;6] if unknown.
    pub addr: [u8; 6],
    /// Whether we have finished reading the point list.
    pub points_loaded: bool,
}

impl DeviceEntry {
    pub const fn empty() -> Self {
        Self {
            device_id: 0,
            name: String::new(),
            addr: [0u8; 6],
            points_loaded: false,
        }
    }
}

/// A single point (object) in a device.
#[derive(Clone)]
pub struct PointEntry {
    /// Object identifier.
    pub object_id: ObjectId,
    /// Present value (None until first read).
    pub present_value: Option<BacnetValue>,
    /// Object name.
    pub name: String<32>,
    /// Whether the value changed since last SSE push.
    pub dirty: bool,
}

impl PointEntry {
    pub fn new(object_id: ObjectId) -> Self {
        Self {
            object_id,
            present_value: None,
            name: String::new(),
            dirty: false,
        }
    }
}

/// Inner state guarded by the mutex.
pub struct BridgeStateInner {
    /// Known devices (device_id == 0 means slot empty).
    pub devices: [DeviceEntry; MAX_DEVICES],
    pub device_count: usize,
    /// Points for each device slot (parallel array).
    pub points: [[Option<PointEntry>; MAX_POINTS]; MAX_DEVICES],
    pub point_counts: [usize; MAX_DEVICES],
}

impl BridgeStateInner {
    pub const fn new() -> Self {
        // We need const initialisation without Default impls — use manual zero init.
        // SAFETY: all fields are valid when zero-initialized (Options are None, usize are 0,
        //         arrays of primitives are zeroed).
        Self {
            devices: [const { DeviceEntry::empty() }; MAX_DEVICES],
            device_count: 0,
            points: [const { [const { None }; MAX_POINTS] }; MAX_DEVICES],
            point_counts: [0usize; MAX_DEVICES],
        }
    }

    /// Find the slot index for a given device ID, or None.
    pub fn find_device(&self, device_id: u32) -> Option<usize> {
        self.devices[..self.device_count]
            .iter()
            .position(|d| d.device_id == device_id)
    }

    /// Register a newly discovered device. Returns its slot index.
    /// If already known, returns the existing slot.
    pub fn upsert_device(&mut self, device_id: u32, addr: [u8; 6]) -> usize {
        if let Some(idx) = self.find_device(device_id) {
            return idx;
        }
        if self.device_count >= MAX_DEVICES {
            // Overwrite oldest slot (slot 0) as a simple eviction policy
            self.devices[0].device_id = device_id;
            self.devices[0].addr = addr;
            self.devices[0].points_loaded = false;
            self.point_counts[0] = 0;
            return 0;
        }
        let idx = self.device_count;
        self.devices[idx].device_id = device_id;
        self.devices[idx].addr = addr;
        self.devices[idx].points_loaded = false;
        self.device_count += 1;
        idx
    }

    /// Update a point value for a device. Creates the slot if needed.
    pub fn update_point(&mut self, device_idx: usize, object_id: ObjectId, value: BacnetValue) {
        if device_idx >= MAX_DEVICES {
            return;
        }
        let count = self.point_counts[device_idx];
        // Find existing slot
        for i in 0..count {
            if let Some(ref mut p) = self.points[device_idx][i] {
                if p.object_id == object_id {
                    p.present_value = Some(value);
                    p.dirty = true;
                    return;
                }
            }
        }
        // Add new slot
        if count < MAX_POINTS {
            self.points[device_idx][count] = Some(PointEntry {
                object_id,
                present_value: Some(value),
                name: String::new(),
                dirty: true,
            });
            self.point_counts[device_idx] += 1;
        }
    }
}

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
