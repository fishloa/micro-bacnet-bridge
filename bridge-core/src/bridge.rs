//! Bridge state: discovered BACnet devices and their points.
//!
//! This module is platform-independent and compiles on both the RP2040 target
//! and the host (for unit testing). It contains no hardware or embassy
//! dependencies.
//!
//! # Design
//!
//! `BridgeStateInner` owns two fixed-size arrays (devices + points), sized by
//! the compile-time constants [`MAX_DEVICES`] and [`MAX_POINTS_PER_DEVICE`].
//! The firmware wraps it in an embassy `Mutex` for async-safe access; the
//! bridge-core tests use it directly (single-threaded).

use crate::bacnet::{BacnetValue, ObjectId};
use heapless::String;

// ---------------------------------------------------------------------------
// Capacity constants
// ---------------------------------------------------------------------------

/// Maximum number of BACnet devices tracked simultaneously.
pub const MAX_DEVICES: usize = 16;

/// Maximum number of points (objects) tracked per device.
pub const MAX_POINTS_PER_DEVICE: usize = 128;

// ---------------------------------------------------------------------------
// DeviceEntry
// ---------------------------------------------------------------------------

/// A discovered BACnet device.
#[derive(Clone)]
pub struct DeviceEntry {
    /// BACnet device instance number. `0` means the slot is empty.
    pub device_id: u32,
    /// Object_Name property value (empty if not yet read).
    pub name: String<32>,
    /// Vendor_Name property value (empty if not yet read).
    pub vendor: String<32>,
    /// MS/TP MAC address (0–127). `0xFF` if device is BACnet/IP-only.
    pub mac: u8,
    /// Whether the device has responded recently.
    pub online: bool,
    /// Uptime seconds when last heard from (approximate).
    pub last_seen: u32,
    /// Source IP:port as 4+2 bytes (BACnet/IP address). `[0; 6]` if unknown.
    pub addr: [u8; 6],
    /// Whether we have finished reading the point list for this device.
    pub points_loaded: bool,
}

impl DeviceEntry {
    /// Construct an empty (unused) device slot.
    pub const fn empty() -> Self {
        Self {
            device_id: 0,
            name: String::new(),
            vendor: String::new(),
            mac: 0xFF,
            online: false,
            last_seen: 0,
            addr: [0u8; 6],
            points_loaded: false,
        }
    }
}

// ---------------------------------------------------------------------------
// PointEntry
// ---------------------------------------------------------------------------

/// A single BACnet object (point) inside a device.
#[derive(Clone)]
pub struct PointEntry {
    /// Object identifier (type + instance number).
    pub object_id: ObjectId,
    /// Object_Name property.
    pub name: String<32>,
    /// Description property (may be empty).
    pub description: String<64>,
    /// Present_Value, `None` until first successful read.
    pub present_value: Option<BacnetValue>,
    /// Engineering units code from the device (0 = no-units).
    pub unit: u16,
    /// Whether Present_Value can be written (commandable or writable).
    pub writable: bool,
    /// Set whenever `present_value` is updated; cleared by [`BridgeStateInner::mark_clean`].
    pub dirty: bool,
}

impl PointEntry {
    /// Construct a minimal point entry (value not yet read).
    pub fn new(object_id: ObjectId) -> Self {
        Self {
            object_id,
            name: String::new(),
            description: String::new(),
            present_value: None,
            unit: 0,
            writable: false,
            dirty: false,
        }
    }
}

// ---------------------------------------------------------------------------
// BridgeStateInner
// ---------------------------------------------------------------------------

/// The inner (lock-free) bridge state.
///
/// On the firmware this is wrapped in `embassy_sync::mutex::Mutex`; for
/// host-side unit tests it is used directly.
pub struct BridgeStateInner {
    /// Known devices. Slots with `device_id == 0` are empty.
    pub devices: [DeviceEntry; MAX_DEVICES],
    /// Number of occupied device slots.
    pub device_count: usize,
    /// Points for each device slot (parallel array indexed by device slot).
    pub points: [[Option<PointEntry>; MAX_POINTS_PER_DEVICE]; MAX_DEVICES],
    /// Number of occupied point slots per device.
    pub point_counts: [usize; MAX_DEVICES],
}

impl Default for BridgeStateInner {
    fn default() -> Self {
        Self::new()
    }
}

impl BridgeStateInner {
    /// Construct a fully zeroed, empty state.
    ///
    /// This is `const` so it can initialise a `static`.
    pub const fn new() -> Self {
        Self {
            devices: [const { DeviceEntry::empty() }; MAX_DEVICES],
            device_count: 0,
            points: [const { [const { None }; MAX_POINTS_PER_DEVICE] }; MAX_DEVICES],
            point_counts: [0usize; MAX_DEVICES],
        }
    }

    // -----------------------------------------------------------------------
    // Device helpers
    // -----------------------------------------------------------------------

    /// Find the slot index for a given device ID, or `None` if not tracked.
    pub fn find_device(&self, device_id: u32) -> Option<usize> {
        self.devices[..self.device_count]
            .iter()
            .position(|d| d.device_id == device_id)
    }

    /// Register a newly discovered device and return its slot index.
    ///
    /// If the device is already tracked the existing slot index is returned
    /// and `mac` / `addr` are updated. If the table is full the oldest slot
    /// (index 0) is evicted.
    ///
    /// `name` may be an empty string; callers can update it later once
    /// `ReadProperty` Object_Name succeeds.
    pub fn upsert_device(&mut self, device_id: u32, mac: u8, name: &str) -> usize {
        // Already tracked — update metadata and return existing slot.
        if let Some(idx) = self.find_device(device_id) {
            self.devices[idx].mac = mac;
            if !name.is_empty() {
                self.devices[idx].name = String::new();
                for ch in name.chars() {
                    let _ = self.devices[idx].name.push(ch);
                }
            }
            return idx;
        }

        // Find a free slot or evict slot 0.
        let idx = if self.device_count < MAX_DEVICES {
            let i = self.device_count;
            self.device_count += 1;
            i
        } else {
            // Evict oldest (simplest policy: always slot 0).
            self.point_counts[0] = 0;
            for slot in self.points[0].iter_mut() {
                *slot = None;
            }
            0
        };

        self.devices[idx] = DeviceEntry::empty();
        self.devices[idx].device_id = device_id;
        self.devices[idx].mac = mac;
        if !name.is_empty() {
            for ch in name.chars() {
                let _ = self.devices[idx].name.push(ch);
            }
        }
        idx
    }

    // -----------------------------------------------------------------------
    // Point helpers
    // -----------------------------------------------------------------------

    /// Update a point value for a device slot.
    ///
    /// If the point does not yet exist it is created. Sets the `dirty` flag.
    /// If the point table is full for this device the update is silently
    /// dropped.
    pub fn update_point(
        &mut self,
        device_idx: usize,
        object_id: ObjectId,
        value: BacnetValue,
        unit: u16,
    ) {
        if device_idx >= MAX_DEVICES {
            return;
        }
        let count = self.point_counts[device_idx];

        // Update existing entry.
        for i in 0..count {
            if let Some(ref mut p) = self.points[device_idx][i] {
                if p.object_id == object_id {
                    p.present_value = Some(value);
                    p.unit = unit;
                    p.dirty = true;
                    return;
                }
            }
        }

        // Create new entry.
        if count < MAX_POINTS_PER_DEVICE {
            self.points[device_idx][count] = Some(PointEntry {
                object_id,
                name: String::new(),
                description: String::new(),
                present_value: Some(value),
                unit,
                writable: false,
                dirty: true,
            });
            self.point_counts[device_idx] += 1;
        }
    }

    /// Return the point slice for a device (may contain `None` holes).
    pub fn get_device_points(&self, device_idx: usize) -> &[Option<PointEntry>] {
        if device_idx >= MAX_DEVICES {
            return &[];
        }
        &self.points[device_idx][..self.point_counts[device_idx]]
    }

    /// Clear the `dirty` flag on a specific point after it has been pushed
    /// to SSE / MQTT subscribers.
    pub fn mark_clean(&mut self, device_idx: usize, point_idx: usize) {
        if device_idx >= MAX_DEVICES || point_idx >= MAX_POINTS_PER_DEVICE {
            return;
        }
        if let Some(ref mut p) = self.points[device_idx][point_idx] {
            p.dirty = false;
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bacnet::{BacnetValue, ObjectId, ObjectType};

    fn make_object_id(t: ObjectType, i: u32) -> ObjectId {
        ObjectId {
            object_type: t,
            instance: i,
        }
    }

    // -----------------------------------------------------------------------
    // upsert_device
    // -----------------------------------------------------------------------

    #[test]
    fn upsert_device_new() {
        let mut s = BridgeStateInner::new();
        let idx = s.upsert_device(42, 3, "Room Sensor");
        assert_eq!(idx, 0);
        assert_eq!(s.device_count, 1);
        assert_eq!(s.devices[0].device_id, 42);
        assert_eq!(s.devices[0].mac, 3);
        assert_eq!(s.devices[0].name.as_str(), "Room Sensor");
    }

    #[test]
    fn upsert_device_existing_returns_same_slot() {
        let mut s = BridgeStateInner::new();
        let i0 = s.upsert_device(7, 0, "");
        let i1 = s.upsert_device(7, 1, "Updated");
        assert_eq!(i0, i1);
        assert_eq!(s.device_count, 1);
        assert_eq!(s.devices[i0].mac, 1);
        assert_eq!(s.devices[i0].name.as_str(), "Updated");
    }

    #[test]
    fn upsert_device_multiple_unique() {
        let mut s = BridgeStateInner::new();
        for id in 1..=4u32 {
            s.upsert_device(id, id as u8, "");
        }
        assert_eq!(s.device_count, 4);
        for id in 1..=4u32 {
            assert!(s.find_device(id).is_some());
        }
    }

    #[test]
    fn upsert_device_overflow_evicts_slot_zero() {
        let mut s = BridgeStateInner::new();
        // Fill the table.
        for id in 1..=(MAX_DEVICES as u32) {
            s.upsert_device(id, 0, "");
        }
        assert_eq!(s.device_count, MAX_DEVICES);
        // One more device — should evict slot 0.
        let overflow_id = 9999u32;
        let evict_idx = s.upsert_device(overflow_id, 5, "Evicted");
        assert_eq!(evict_idx, 0);
        assert_eq!(s.devices[0].device_id, overflow_id);
        // Total count does not grow past MAX_DEVICES.
        assert_eq!(s.device_count, MAX_DEVICES);
    }

    #[test]
    fn upsert_device_empty_name_preserved() {
        let mut s = BridgeStateInner::new();
        s.upsert_device(1, 0, "Initial");
        // Update with empty name — existing name should be preserved.
        s.upsert_device(1, 0, "");
        assert_eq!(s.devices[0].name.as_str(), "Initial");
    }

    // -----------------------------------------------------------------------
    // update_point
    // -----------------------------------------------------------------------

    #[test]
    fn update_point_creates_entry() {
        let mut s = BridgeStateInner::new();
        let dev_idx = s.upsert_device(1, 0, "");
        let oid = make_object_id(ObjectType::AnalogInput, 0);
        s.update_point(dev_idx, oid, BacnetValue::Real(21.5), 62); // 62 = deg-C
        assert_eq!(s.point_counts[dev_idx], 1);
        let p = s.points[dev_idx][0].as_ref().unwrap();
        assert_eq!(p.object_id, oid);
        assert_eq!(p.present_value, Some(BacnetValue::Real(21.5)));
        assert_eq!(p.unit, 62);
        assert!(p.dirty);
    }

    #[test]
    fn update_point_updates_existing() {
        let mut s = BridgeStateInner::new();
        let dev_idx = s.upsert_device(1, 0, "");
        let oid = make_object_id(ObjectType::AnalogInput, 0);
        s.update_point(dev_idx, oid, BacnetValue::Real(20.0), 62);
        s.update_point(dev_idx, oid, BacnetValue::Real(25.0), 62);
        // Should not create a second slot.
        assert_eq!(s.point_counts[dev_idx], 1);
        let p = s.points[dev_idx][0].as_ref().unwrap();
        assert_eq!(p.present_value, Some(BacnetValue::Real(25.0)));
    }

    #[test]
    fn update_point_dirty_flag_set() {
        let mut s = BridgeStateInner::new();
        let dev_idx = s.upsert_device(1, 0, "");
        let oid = make_object_id(ObjectType::BinaryInput, 1);
        s.update_point(dev_idx, oid, BacnetValue::Boolean(true), 0);
        assert!(s.points[dev_idx][0].as_ref().unwrap().dirty);
    }

    #[test]
    fn update_point_out_of_bounds_device_is_noop() {
        let mut s = BridgeStateInner::new();
        let oid = make_object_id(ObjectType::AnalogValue, 0);
        // Should not panic.
        s.update_point(MAX_DEVICES + 1, oid, BacnetValue::Real(0.0), 0);
    }

    // -----------------------------------------------------------------------
    // mark_clean
    // -----------------------------------------------------------------------

    #[test]
    fn mark_clean_clears_dirty_flag() {
        let mut s = BridgeStateInner::new();
        let dev_idx = s.upsert_device(1, 0, "");
        let oid = make_object_id(ObjectType::AnalogInput, 0);
        s.update_point(dev_idx, oid, BacnetValue::Real(1.0), 0);
        // Dirty after update.
        assert!(s.points[dev_idx][0].as_ref().unwrap().dirty);
        s.mark_clean(dev_idx, 0);
        // Clean after mark_clean.
        assert!(!s.points[dev_idx][0].as_ref().unwrap().dirty);
    }

    #[test]
    fn mark_clean_out_of_bounds_is_noop() {
        let mut s = BridgeStateInner::new();
        s.mark_clean(MAX_DEVICES + 1, 0);
        s.mark_clean(0, MAX_POINTS_PER_DEVICE + 1);
    }

    // -----------------------------------------------------------------------
    // get_device_points
    // -----------------------------------------------------------------------

    #[test]
    fn get_device_points_returns_filled_slice() {
        let mut s = BridgeStateInner::new();
        let dev_idx = s.upsert_device(1, 0, "");
        for i in 0..3u32 {
            s.update_point(
                dev_idx,
                make_object_id(ObjectType::AnalogInput, i),
                BacnetValue::Real(i as f32),
                0,
            );
        }
        let pts = s.get_device_points(dev_idx);
        assert_eq!(pts.len(), 3);
        assert!(pts.iter().all(|p| p.is_some()));
    }

    #[test]
    fn get_device_points_out_of_bounds_empty() {
        let s = BridgeStateInner::new();
        assert!(s.get_device_points(MAX_DEVICES + 1).is_empty());
    }

    // -----------------------------------------------------------------------
    // MAX_POINTS_PER_DEVICE overflow
    // -----------------------------------------------------------------------

    #[test]
    fn update_point_table_full_does_not_panic() {
        let mut s = BridgeStateInner::new();
        let dev_idx = s.upsert_device(1, 0, "");
        // Fill the table.
        for i in 0..MAX_POINTS_PER_DEVICE as u32 {
            s.update_point(
                dev_idx,
                make_object_id(ObjectType::AnalogInput, i),
                BacnetValue::Real(i as f32),
                0,
            );
        }
        assert_eq!(s.point_counts[dev_idx], MAX_POINTS_PER_DEVICE);
        // One more — must not panic and count must not exceed capacity.
        s.update_point(
            dev_idx,
            make_object_id(ObjectType::AnalogInput, MAX_POINTS_PER_DEVICE as u32 + 1),
            BacnetValue::Real(0.0),
            0,
        );
        assert_eq!(s.point_counts[dev_idx], MAX_POINTS_PER_DEVICE);
    }
}
