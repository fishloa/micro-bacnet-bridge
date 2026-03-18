/**
 * @file ipc_c.c
 * @brief C-side implementation of the inter-core IPC ring buffer.
 *
 * Provides a lock-free single-producer / single-consumer (SPSC) ring buffer
 * for passing BACnet PDUs between Core 0 (Rust, BACnet/IP) and Core 1 (C,
 * MS/TP).  The two ring buffers are owned by Rust and declared extern here:
 *
 *   mstp_to_ip_ring — Core 1 writes, Core 0 reads.
 *   ip_to_mstp_ring — Core 0 writes, Core 1 reads.
 *
 * Safety:
 *   Each ring buffer has exactly one producer and one consumer running on
 *   separate cores.  ARM Data Memory Barrier (DMB) instructions are used to
 *   ensure that data writes complete before index updates become visible to
 *   the remote core, and that index reads are not speculated past the data
 *   read.  No spinlock or hardware mutex is required for this SPSC pattern.
 *
 * @author Icomb Place
 * @copyright SPDX-License-Identifier: MIT
 */

#include <stdint.h>
#include <stdbool.h>

#include "bacnet_bridge.h"

/*
 * Compile-time struct size assertion.
 *
 * bacnet_pdu_t layout on Cortex-M0+ (4-byte pointer ABI, packed to natural alignment):
 *   source_net(2) + source_mac(7) + source_mac_len(1)         = 10 bytes @ offset 0
 *   dest_net(2) [+0 pad, aligned to 2] + dest_mac(7) + dest_mac_len(1) = 10 bytes @ offset 10
 *   pdu_type(1) + [1 pad byte] + data_len(2) [aligned to 2]  =  4 bytes @ offset 20
 *   data[501]                                                  = 501 bytes @ offset 24
 *   + 1 trailing pad byte to round total to struct alignment (2)
 *   Total = 526 bytes.
 *
 * This must match the Rust BacnetPdu compile-time assertion in bridge-core/src/ipc.rs.
 */
_Static_assert(sizeof(bacnet_pdu_t) == 526,
               "bacnet_pdu_t size mismatch with Rust BacnetPdu — update both assertions");

/* Freestanding memcpy — no libc available on bare-metal Cortex-M0+. */
static void ipc_memcpy(void *dst, const void *src, uint32_t n) {
    uint8_t *d = (uint8_t *)dst;
    const uint8_t *s = (const uint8_t *)src;
    while (n--) { *d++ = *s++; }
}

/* --------------------------------------------------------------------------
 * Shared ring buffer definitions
 *
 * These are declared `extern` in bacnet_bridge.h and actually allocated in
 * Rust (src/ipc.rs) in a linker section shared between both cores.  The
 * extern declarations here satisfy the C linker; the actual symbols come from
 * the Rust object linked at the final link step.
 * -------------------------------------------------------------------------- */

/* mstp_to_ip_ring and ip_to_mstp_ring are defined in Rust. */

/* --------------------------------------------------------------------------
 * Internal helpers
 * -------------------------------------------------------------------------- */

/**
 * @brief Data Memory Barrier — ensures all memory accesses before this point
 * are visible to other observers (including the second core) before any
 * accesses after this point.
 */
static inline void dmb(void)
{
    __asm__ volatile ("dmb" : : : "memory");
}

/**
 * @brief Compute the ring buffer occupancy (number of entries queued).
 * @param ring  Ring buffer to inspect.
 * @return Number of entries currently in the buffer.
 */
static inline uint32_t ring_count(const ipc_ring_t *ring)
{
    /* Unsigned wraparound arithmetic gives the correct answer even when
       head has wrapped past UINT32_MAX. */
    return ring->head - ring->tail;
}

/* --------------------------------------------------------------------------
 * Public API
 * -------------------------------------------------------------------------- */

/**
 * @brief Test whether the ring buffer contains no entries.
 *
 * Safe to call from either core.
 *
 * @param ring  Pointer to the ring buffer.
 * @return true if empty.
 */
bool ipc_ring_is_empty(const ipc_ring_t *ring)
{
    return (ring->head == ring->tail);
}

/**
 * @brief Test whether the ring buffer is at capacity.
 *
 * Safe to call from either core.
 *
 * @param ring  Pointer to the ring buffer.
 * @return true if full.
 */
bool ipc_ring_is_full(const ipc_ring_t *ring)
{
    return (ring_count(ring) >= IPC_RING_SIZE);
}

/**
 * @brief Push a PDU onto the ring buffer (producer side).
 *
 * Must only be called from the designated producer core for this buffer:
 *   - mstp_to_ip_ring: Core 1 only.
 *   - ip_to_mstp_ring: Core 0 only.
 *
 * The PDU is copied by value into the buffer slot.  A DMB is issued after the
 * data copy and before the head index update so that the consumer never sees
 * an updated head pointing at stale data.
 *
 * @param ring  Pointer to the ring buffer to write.
 * @param pdu   Pointer to the PDU to enqueue (copied in).
 * @return true  if the PDU was enqueued successfully.
 * @return false if the ring buffer is full (PDU is silently dropped).
 */
bool ipc_ring_push(ipc_ring_t *ring, const bacnet_pdu_t *pdu)
{
    uint32_t head;
    uint32_t slot;

    if (ipc_ring_is_full(ring)) {
        return false;
    }

    head = ring->head;
    slot = head % IPC_RING_SIZE;

    /* Copy PDU data into the slot. */
    ipc_memcpy(&ring->buffer[slot], pdu, sizeof(bacnet_pdu_t));

    /* DMB: ensure the data write completes before the head update is visible
       to the consumer on the other core. */
    dmb();

    /* Advance the head.  Unsigned overflow is defined and harmless. */
    ring->head = head + 1u;

    return true;
}

/**
 * @brief Pop a PDU from the ring buffer (consumer side).
 *
 * Must only be called from the designated consumer core for this buffer:
 *   - mstp_to_ip_ring: Core 0 only.
 *   - ip_to_mstp_ring: Core 1 only.
 *
 * A DMB is issued before reading the data slot to ensure that any data writes
 * from the producer (on the other core) are visible before we read them.
 *
 * @param ring  Pointer to the ring buffer to read.
 * @param pdu   Output buffer populated with the dequeued PDU on success.
 * @return true  if a PDU was dequeued.
 * @return false if the ring buffer is empty (pdu is not modified).
 */
bool ipc_ring_pop(ipc_ring_t *ring, bacnet_pdu_t *pdu)
{
    uint32_t tail;
    uint32_t slot;

    if (ipc_ring_is_empty(ring)) {
        return false;
    }

    tail = ring->tail;
    slot = tail % IPC_RING_SIZE;

    /* DMB: ensure producer's data write is observable before we read. */
    dmb();

    /* Copy PDU data out of the slot. */
    ipc_memcpy(pdu, &ring->buffer[slot], sizeof(bacnet_pdu_t));

    /* DMB: ensure data read completes before we update tail, preventing the
       compiler or CPU from reordering the tail store before the data copy. */
    dmb();

    /* Advance the tail. */
    ring->tail = tail + 1u;

    return true;
}
