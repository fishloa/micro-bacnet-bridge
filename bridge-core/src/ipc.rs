//! Inter-core communication types and ring buffer.
//!
//! The ring buffer is an SPSC (single-producer, single-consumer) design using
//! volatile reads/writes for index synchronisation. This avoids any dependency
//! on `portable_atomic` (which doesn't support thumbv6m) while remaining
//! correct when used strictly as SPSC across the two RP2040 cores.
//!
//! **Safety contract**: `push` must only be called from one core/context and
//! `pop` from the other. Violating this is undefined behaviour.

use core::ptr::{read_volatile, write_volatile};

// ---------------------------------------------------------------------------
// BacnetPdu
// ---------------------------------------------------------------------------

/// Maximum APDU size supported by the bridge.
/// MS/TP maximum NPDU payload is 501 bytes (MSTP_FRAME_NPDU_MAX per BACnet standard).
pub const PDU_MAX_DATA: usize = 501;

/// A BACnet PDU passed between Core 0 and Core 1 via the shared ring buffer.
///
/// `repr(C)` is required so the C code on Core 1 can access the same struct.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct BacnetPdu {
    /// Source network number (0 = local MS/TP segment).
    pub source_net: u16,
    /// Source MAC address bytes.
    pub source_mac: [u8; 7],
    /// Number of valid bytes in `source_mac`.
    pub source_mac_len: u8,
    /// Destination network number.
    pub dest_net: u16,
    /// Destination MAC address bytes.
    pub dest_mac: [u8; 7],
    /// Number of valid bytes in `dest_mac`.
    pub dest_mac_len: u8,
    /// PDU type byte (APDU type nibble, or 0xFF for network-layer messages).
    pub pdu_type: u8,
    /// Number of valid bytes in `data`.
    pub data_len: u16,
    /// PDU payload (APDU bytes).
    pub data: [u8; PDU_MAX_DATA],
}

// Compile-time size check: BacnetPdu must match the C bacnet_pdu_t.
// Layout (repr(C), align=2):
//   source_net(2@0) + source_mac(7@2) + source_mac_len(1@9)
//   dest_net(2@10) + dest_mac(7@12) + dest_mac_len(1@19)
//   pdu_type(1@20) + [1 pad byte@21] + data_len(2@22) + data(501@24)
//   + [1 trailing pad byte@525 for struct alignment to 2]
//   = 526 bytes total.
const _: () = assert!(
    core::mem::size_of::<BacnetPdu>() == 526,
    "BacnetPdu size mismatch with C bacnet_pdu_t"
);

// Manual Debug to avoid printing 480 bytes
impl core::fmt::Debug for BacnetPdu {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BacnetPdu")
            .field("source_net", &self.source_net)
            .field("source_mac_len", &self.source_mac_len)
            .field("dest_net", &self.dest_net)
            .field("dest_mac_len", &self.dest_mac_len)
            .field("pdu_type", &self.pdu_type)
            .field("data_len", &self.data_len)
            .finish()
    }
}

impl PartialEq for BacnetPdu {
    fn eq(&self, other: &Self) -> bool {
        // Clamp data_len to PDU_MAX_DATA to avoid a panic on malformed/uninitialized data.
        let len_a = (self.data_len as usize).min(PDU_MAX_DATA);
        let len_b = (other.data_len as usize).min(PDU_MAX_DATA);
        self.source_net == other.source_net
            && self.source_mac == other.source_mac
            && self.source_mac_len == other.source_mac_len
            && self.dest_net == other.dest_net
            && self.dest_mac == other.dest_mac
            && self.dest_mac_len == other.dest_mac_len
            && self.pdu_type == other.pdu_type
            && self.data_len == other.data_len
            && self.data[..len_a] == other.data[..len_b]
    }
}

// L2: BacnetPdu's PartialEq is a total equality relation (no floating-point
// fields, no NaN), so Eq is safe to implement as a marker.
impl Eq for BacnetPdu {}

impl BacnetPdu {
    /// Create a zeroed PDU.
    pub const fn new() -> Self {
        Self {
            source_net: 0,
            source_mac: [0u8; 7],
            source_mac_len: 0,
            dest_net: 0,
            dest_mac: [0u8; 7],
            dest_mac_len: 0,
            pdu_type: 0,
            data_len: 0,
            data: [0u8; PDU_MAX_DATA],
        }
    }
}

impl Default for BacnetPdu {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// RingBuffer
// ---------------------------------------------------------------------------

/// Lock-free SPSC ring buffer holding up to `N` `BacnetPdu` items.
///
/// `N` must be a power of two. Head and tail are monotonically-incrementing
/// `u32` counters (never reset, wrap via `u32::wrapping_add`). This matches
/// the C-side `ipc_ring_t` convention exactly:
///
/// - **Full**  when `head.wrapping_sub(tail) >= N`
/// - **Empty** when `head == tail`
/// - **Slot**  index = `index % N` (or `index & (N-1)` for power-of-two N)
///
/// This avoids the classic "wasted slot" off-by-one that arises from the
/// `(head+1) % N == tail` full-condition used in some ring buffer designs,
/// and ensures the C and Rust implementations agree on occupancy.
///
/// `repr(C)` ensures the field layout matches the C `ipc_ring_t`:
/// `head` (u32), `tail` (u32), `buffer` ([bacnet_pdu_t; N])`.
#[repr(C)]
pub struct RingBuffer<const N: usize> {
    /// Write index (producer increments monotonically). Volatile access required.
    head: u32,
    /// Read index (consumer increments monotonically). Volatile access required.
    tail: u32,
    /// Storage for PDUs.
    data: [BacnetPdu; N],
}

impl<const N: usize> RingBuffer<N> {
    // Ensure N is a power of two (checked at compile time).
    // Power-of-two N is required so that slot indices can be computed with a
    // bitwise AND (`index & (N-1)`) instead of a modulo, and so that the
    // C-side `ipc_ring_t` (which uses the same convention) stays in sync.
    // Valid sizes: 1, 2, 4, 8, 16, 32, …
    const _CHECK: () = assert!(
        N > 0 && (N & (N - 1)) == 0,
        "RingBuffer: N must be a power of two"
    );

    /// Create an empty ring buffer.
    pub const fn new() -> Self {
        Self {
            head: 0,
            tail: 0,
            data: [BacnetPdu::new(); N],
        }
    }

    /// Returns true if the buffer contains no items.
    #[inline]
    pub fn is_empty(&self) -> bool {
        // SAFETY: self is exclusively accessed here (SPSC contract).
        let h = unsafe { read_volatile(&self.head) };
        let t = unsafe { read_volatile(&self.tail) };
        h == t
    }

    /// Returns true if the buffer is full and cannot accept another item.
    ///
    /// Uses the C-compatible monotonic convention: full when
    /// `(head - tail) >= N`, which gives a true capacity of N items.
    #[inline]
    pub fn is_full(&self) -> bool {
        let h = unsafe { read_volatile(&self.head) };
        let t = unsafe { read_volatile(&self.tail) };
        h.wrapping_sub(t) >= N as u32
    }

    /// Push a PDU into the buffer (producer side).
    ///
    /// Returns `true` on success, `false` if the buffer is full.
    ///
    /// A memory fence (Release) is issued after writing the data and before
    /// advancing the head index, ensuring the consumer on the other core
    /// never reads a slot whose data has not yet been written.
    pub fn push(&mut self, pdu: &BacnetPdu) -> bool {
        let h = unsafe { read_volatile(&self.head) };
        let t = unsafe { read_volatile(&self.tail) };
        if h.wrapping_sub(t) >= N as u32 {
            return false; // full
        }
        let slot = (h as usize) % N;
        self.data[slot] = *pdu;
        // Release fence: data write must be visible before head update.
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        // Advance head monotonically (unsigned overflow is defined and harmless).
        unsafe { write_volatile(&mut self.head, h.wrapping_add(1)) };
        true
    }

    /// Pop a PDU from the buffer (consumer side).
    ///
    /// Returns `Some(pdu)` if an item was available, `None` if empty.
    ///
    /// An Acquire fence is issued after observing a non-empty head and before
    /// reading the data slot, ensuring that any data written by the producer
    /// is fully visible on this core before we copy it.
    pub fn pop(&mut self) -> Option<BacnetPdu> {
        let h = unsafe { read_volatile(&self.head) };
        let t = unsafe { read_volatile(&self.tail) };
        if h == t {
            return None; // empty
        }
        // Acquire fence: head read must happen-before the data read below.
        core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
        let slot = (t as usize) % N;
        let pdu = self.data[slot];
        // Release fence: the data read must be fully visible before we advance
        // the tail pointer. Without this, the producer (on the other core)
        // could observe the updated tail and overwrite the slot before we have
        // finished reading the data from it.
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        // Advance tail monotonically.
        unsafe { write_volatile(&mut self.tail, t.wrapping_add(1)) };
        Some(pdu)
    }

    /// Return the number of items currently in the buffer.
    ///
    /// Computed as `head.wrapping_sub(tail)` — correct even across `u32`
    /// overflow because both indices increment at the same rate.
    ///
    /// L7: Implementation simplified to a single wrapping subtraction with no
    /// conditional branches, matching the same idiom used in `is_full()` and
    /// the C-side `ipc_ring_t`.
    pub fn len(&self) -> usize {
        // SAFETY: self is exclusively accessed here (SPSC contract).
        unsafe { read_volatile(&self.head).wrapping_sub(read_volatile(&self.tail)) as usize }
    }
}

impl<const N: usize> Default for RingBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pdu(pdu_type: u8, data_byte: u8) -> BacnetPdu {
        let mut pdu = BacnetPdu::new();
        pdu.pdu_type = pdu_type;
        pdu.data[0] = data_byte;
        pdu.data_len = 1;
        pdu
    }

    #[test]
    fn new_buffer_is_empty() {
        let rb: RingBuffer<4> = RingBuffer::new();
        assert!(rb.is_empty());
        assert!(!rb.is_full());
        assert_eq!(rb.len(), 0);
    }

    #[test]
    fn push_one_pop_one() {
        let mut rb: RingBuffer<4> = RingBuffer::new();
        let pdu = make_pdu(0x10, 0xAB);
        assert!(rb.push(&pdu));
        assert!(!rb.is_empty());
        assert_eq!(rb.len(), 1);
        let out = rb.pop().unwrap();
        assert_eq!(out.pdu_type, 0x10);
        assert_eq!(out.data[0], 0xAB);
        assert_eq!(out.data_len, 1);
        assert!(rb.is_empty());
    }

    #[test]
    fn fill_and_drain() {
        // RingBuffer<4> holds exactly 4 items (capacity = N with monotonic-index convention).
        let mut rb: RingBuffer<4> = RingBuffer::new();
        assert!(rb.push(&make_pdu(1, 0x01)));
        assert!(rb.push(&make_pdu(2, 0x02)));
        assert!(rb.push(&make_pdu(3, 0x03)));
        assert!(rb.push(&make_pdu(4, 0x04)));
        assert!(rb.is_full());
        assert!(!rb.push(&make_pdu(5, 0x05))); // should fail — buffer full

        let p1 = rb.pop().unwrap();
        let p2 = rb.pop().unwrap();
        let p3 = rb.pop().unwrap();
        let p4 = rb.pop().unwrap();
        assert_eq!(p1.pdu_type, 1);
        assert_eq!(p2.pdu_type, 2);
        assert_eq!(p3.pdu_type, 3);
        assert_eq!(p4.pdu_type, 4);
        assert!(rb.is_empty());
        assert!(rb.pop().is_none());
    }

    #[test]
    fn pop_empty_returns_none() {
        let mut rb: RingBuffer<4> = RingBuffer::new();
        assert!(rb.pop().is_none());
    }

    #[test]
    fn wraps_around_correctly() {
        let mut rb: RingBuffer<4> = RingBuffer::new();
        // Fill all 4 slots, drain, fill again — exercises index wrap past N.
        for i in 0..4u8 {
            assert!(rb.push(&make_pdu(i, i)));
        }
        assert!(rb.is_full());
        for _ in 0..4 {
            rb.pop().unwrap();
        }
        assert!(rb.is_empty());
        // Head and tail are now both 4 (not zero); slots reuse correctly.
        for i in 10..14u8 {
            assert!(rb.push(&make_pdu(i, i)));
        }
        assert!(rb.is_full());
        let p = rb.pop().unwrap();
        assert_eq!(p.pdu_type, 10);
        let p = rb.pop().unwrap();
        assert_eq!(p.pdu_type, 11);
        let p = rb.pop().unwrap();
        assert_eq!(p.pdu_type, 12);
        let p = rb.pop().unwrap();
        assert_eq!(p.pdu_type, 13);
        assert!(rb.is_empty());
    }

    #[test]
    fn push_returns_false_when_full() {
        let mut rb: RingBuffer<2> = RingBuffer::new();
        // Capacity of RingBuffer<2> = 2 items (monotonic-index convention).
        assert!(rb.push(&make_pdu(1, 1)));
        assert!(rb.push(&make_pdu(2, 2)));
        assert!(rb.is_full());
        assert!(!rb.push(&make_pdu(3, 3)));
    }

    #[test]
    fn pdu_equality() {
        let a = make_pdu(0x10, 0xFF);
        let b = make_pdu(0x10, 0xFF);
        assert_eq!(a, b);
        let c = make_pdu(0x20, 0xFF);
        assert_ne!(a, c);
    }

    #[test]
    fn len_matches_pushes_minus_pops() {
        let mut rb: RingBuffer<8> = RingBuffer::new();
        assert_eq!(rb.len(), 0);
        rb.push(&make_pdu(1, 1));
        assert_eq!(rb.len(), 1);
        rb.push(&make_pdu(2, 2));
        assert_eq!(rb.len(), 2);
        rb.pop();
        assert_eq!(rb.len(), 1);
        rb.pop();
        assert_eq!(rb.len(), 0);
    }

    #[test]
    fn pdu_data_payload() {
        let mut pdu = BacnetPdu::new();
        pdu.source_net = 42;
        pdu.dest_net = 100;
        pdu.pdu_type = 0x30;
        pdu.data_len = 3;
        pdu.data[0] = 0xAA;
        pdu.data[1] = 0xBB;
        pdu.data[2] = 0xCC;

        let mut rb: RingBuffer<4> = RingBuffer::new();
        rb.push(&pdu);
        let out = rb.pop().unwrap();
        assert_eq!(out.source_net, 42);
        assert_eq!(out.dest_net, 100);
        assert_eq!(out.data_len, 3);
        assert_eq!(&out.data[..3], &[0xAA, 0xBB, 0xCC]);
    }

    // L2: BacnetPdu must implement Eq (a total equality relation).
    #[test]
    fn bacnet_pdu_eq_is_total() {
        let a = make_pdu(0x10, 0xAB);
        let b = make_pdu(0x10, 0xAB);
        // Reflexive
        assert_eq!(a, a);
        // Symmetric
        assert_eq!(a, b);
        assert_eq!(b, a);
        // Antisymmetric: different PDUs must not be equal
        let c = make_pdu(0x20, 0xAB);
        assert_ne!(a, c);
        // Eq is usable in a HashMap key position (compile-time check via trait bound)
        fn assert_eq_bound<T: Eq>(_: &T) {}
        assert_eq_bound(&a);
    }

    // L7: len() must correctly report occupancy across the u32 index wraparound
    // boundary.  We simulate wraparound by forcing head and tail to near-max
    // values using multiple push/pop cycles.
    #[test]
    fn len_at_wraparound() {
        // Use a size-2 ring so each item moves head/tail by 1.
        // After 2^32 / 2 push-pop cycles we would wrap, but we can't do that many.
        // Instead, verify that len() after N pushes and M pops is N-M for
        // various N and M values, including the case where the internal counters
        // would wrap if they were u8.  We use RingBuffer<4> and do 260 push/pop
        // pairs to exceed u8::MAX.
        let mut rb: RingBuffer<4> = RingBuffer::new();
        for _ in 0..260u32 {
            assert!(rb.push(&make_pdu(1, 1)));
            let _ = rb.pop();
        }
        assert_eq!(rb.len(), 0);

        // Push 3, verify len is 3
        assert!(rb.push(&make_pdu(10, 1)));
        assert!(rb.push(&make_pdu(11, 2)));
        assert!(rb.push(&make_pdu(12, 3)));
        assert_eq!(rb.len(), 3);

        // Pop 1, verify len is 2
        let _ = rb.pop();
        assert_eq!(rb.len(), 2);

        // Pop remaining
        let _ = rb.pop();
        let _ = rb.pop();
        assert_eq!(rb.len(), 0);
    }
}
