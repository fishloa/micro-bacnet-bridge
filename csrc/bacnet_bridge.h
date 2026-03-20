/**
 * @file bacnet_bridge.h
 * @brief Shared types and declarations for the BACnet MS/TP <-> BACnet/IP bridge.
 *
 * This header is included by both C (Core 1) and, via FFI, Rust (Core 0).
 * It defines the inter-core IPC ring buffer structures and function prototypes
 * for all C modules.
 *
 * The two shared ring buffers are allocated in Rust (in a section accessible
 * from both cores) and declared as extern here so C code can reference them
 * directly without ownership ambiguity.
 *
 * @author Icomb Place
 * @copyright SPDX-License-Identifier: MIT
 */

#ifndef BACNET_BRIDGE_H
#define BACNET_BRIDGE_H

#include <stdint.h>
#include <stdbool.h>

/* platform_rp2350.h defines SIO_BASE and other peripheral addresses used by
 * inline helpers in this header. Include it here so the definitions are
 * always available regardless of the order headers are included by callers. */
#include "platform_rp2350.h"

#ifdef __cplusplus
extern "C" {
#endif

/* --------------------------------------------------------------------------
 * Version
 * -------------------------------------------------------------------------- */

/** Firmware version string embedded in mDNS TXT records and HTTP headers. */
#define BACNET_BRIDGE_VERSION "0.1.0"

/** BACnet vendor identifier (0xFFFF = unregistered). */
#define BACNET_VENDOR_ID 0xFFFF

/** BACnet vendor name string. */
#define BACNET_VENDOR_NAME "Icomb Place"

/* --------------------------------------------------------------------------
 * Shared Core 1 configuration (written by Rust before Core 1 launch)
 * -------------------------------------------------------------------------- */

/**
 * @brief MS/TP configuration passed from Core 0 (Rust) to Core 1 (C).
 *
 * Rust writes this struct before calling spawn_core1().  Core 1 reads it
 * once at startup in core1_entry().
 */
typedef struct {
    /** Baud rate: 9600, 19200, 38400, 76800, or 0 = auto-detect. */
    uint32_t baud_rate;
    /** MS/TP MAC address (0–127). */
    uint8_t mac_address;
    /** Max master (1–127). */
    uint8_t max_master;
    /** Padding for alignment. */
    uint8_t _pad[2];
} mstp_config_t;

/** Global config struct — written by Rust, read by Core 1. */
extern volatile mstp_config_t g_mstp_config;

/**
 * @brief MS/TP serial port status (written by Core 1, read by Core 0).
 *
 * Core 0 reads this for the dashboard SSE stream and config page.
 * All fields are volatile — Core 1 is the sole writer.
 */
typedef struct {
    /** Active baud rate (after auto-detect or manual config). 0 = not yet determined. */
    uint32_t active_baud;
    /** Total valid MS/TP frames received. */
    uint32_t frames_rx;
    /** Total MS/TP frames transmitted. */
    uint32_t frames_tx;
    /** Total receive errors (framing, CRC, overrun). */
    uint32_t errors_rx;
    /** true if valid MS/TP traffic has been seen recently (within last 5s). */
    uint8_t bus_active;
    /** true if auto-detect is in progress. */
    uint8_t detecting;
    /** Parity setting: 0=none, 1=even, 2=odd. Always 0 (8N1) for MS/TP. */
    uint8_t parity;
    uint8_t _pad;
} mstp_status_t;

/** Global status struct — written by Core 1, read by Core 0 via SSE/API. */
extern volatile mstp_status_t g_mstp_status;

/**
 * @brief Flash operation pause flag.
 *
 * Core 0 sets this to 1 before flash operations. Core 1 checks it in its
 * SRAM-resident main loop and spins in SRAM until cleared. This ensures
 * Core 1 never accesses flash (including via ISR) during erase/write.
 *
 * This is necessary because embassy-rp's built-in multicore::pause_core1()
 * uses the SIO_IRQ_FIFO interrupt handler which resides in flash (.text).
 * If Core 1 receives that interrupt while flash is being erased, it faults.
 */
extern volatile uint8_t g_flash_pause_request;
extern volatile uint8_t g_core1_paused;

/**
 * @brief Check and handle flash pause request. Call from any tight loop on Core 1.
 *
 * Must be in .time_critical (SRAM). Disables SIO_IRQ_FIFO so embassy's
 * flash-resident ISR can't fire, then echoes FIFO tokens for embassy's
 * pause/resume protocol.
 */
static inline __attribute__((section(".time_critical"), always_inline))
void core1_check_flash_pause(void)
{
    if (!g_flash_pause_request) return;

    /* Disable SIO_IRQ_FIFO (IRQ 25) — ISR is in flash */
    *(volatile uint32_t *)0xE000E180u = (1u << 25);
    __asm volatile("dsb");
    __asm volatile("isb");

    g_core1_paused = 1;
    while (g_flash_pause_request) {
        /* Echo FIFO tokens for embassy's pause/resume protocol */
        volatile uint32_t *fifo_st = (volatile uint32_t *)(SIO_BASE + 0x050u);
        volatile uint32_t *fifo_rd = (volatile uint32_t *)(SIO_BASE + 0x058u);
        volatile uint32_t *fifo_wr = (volatile uint32_t *)(SIO_BASE + 0x054u);
        if ((*fifo_st) & 1u) {
            uint32_t token = *fifo_rd;
            *fifo_wr = token;
            __asm volatile("sev");
        }
    }
    g_core1_paused = 0;

    /* Re-enable SIO_IRQ_FIFO */
    *(volatile uint32_t *)0xE000E100u = (1u << 25);
}

/* --------------------------------------------------------------------------
 * IPC PDU type tags
 * -------------------------------------------------------------------------- */

/** PDU originated from / destined for the MS/TP network. */
#define PDU_TYPE_MSTP    0x01u

/** PDU originated from / destined for BACnet/IP. */
#define PDU_TYPE_BACNET_IP 0x02u

/** PDU is a control message (not a BACnet PDU). */
#define PDU_TYPE_CONTROL 0xFFu

/* --------------------------------------------------------------------------
 * IPC ring buffer
 * -------------------------------------------------------------------------- */

/**
 * @brief Maximum BACnet NPDU payload length carried over IPC.
 *
 * MS/TP limits data to 501 bytes (MSTP_FRAME_NPDU_MAX per BACnet standard).
 * This value matches the BACnet standard maximum exactly to avoid silent PDU
 * truncation on MS/TP frames that use the full payload capacity.
 */
#define BACNET_PDU_MAX_DATA 501u

/**
 * @brief Single PDU entry carried through the inter-core ring buffer.
 *
 * Fields mirror the BACNET_ADDRESS and BACNET_NPDU structures from bacnet-stack
 * but are flattened to avoid pulling in bacnet-stack headers from the Rust FFI.
 */
typedef struct {
    /** Source network number (0 = local network). */
    uint16_t source_net;

    /** Source MAC address bytes (up to 7 for MS/TP or BACnet/IP). */
    uint8_t source_mac[7];

    /** Number of valid bytes in source_mac (1 for MS/TP, 6 for BACnet/IP). */
    uint8_t source_mac_len;

    /** Destination network number (0 = local, 0xFFFF = broadcast). */
    uint16_t dest_net;

    /** Destination MAC address bytes. */
    uint8_t dest_mac[7];

    /** Number of valid bytes in dest_mac (0 = broadcast). */
    uint8_t dest_mac_len;

    /** PDU type tag — one of PDU_TYPE_* constants above. */
    uint8_t pdu_type;

    /** Number of valid bytes in data[]. */
    uint16_t data_len;

    /** Raw NPDU/APDU payload. */
    uint8_t data[BACNET_PDU_MAX_DATA];
} bacnet_pdu_t;

/**
 * @brief Power-of-two ring buffer depth.
 *
 * Must be a power of 2 so that (head % IPC_RING_SIZE) can be replaced with a
 * bitwise AND on both Rust and C sides if needed.
 */
#define IPC_RING_SIZE 8u

/**
 * @brief Lock-free single-producer / single-consumer ring buffer.
 *
 * Core 1 is the sole producer of mstp_to_ip_ring and the sole consumer of
 * ip_to_mstp_ring.  Core 0 has the inverse roles.  This SPSC property allows
 * a simple head/tail scheme with DMB barriers — no spinlock required.
 *
 * head: index of the next slot to be written (producer advances).
 * tail: index of the next slot to be read  (consumer advances).
 * Full  when: (head - tail) == IPC_RING_SIZE
 * Empty when: head == tail
 */
typedef struct {
    volatile uint32_t head; /**< Producer write index (mod IPC_RING_SIZE). */
    volatile uint32_t tail; /**< Consumer read  index (mod IPC_RING_SIZE). */
    bacnet_pdu_t buffer[IPC_RING_SIZE]; /**< Circular PDU storage. */
} ipc_ring_t;

/* --------------------------------------------------------------------------
 * Shared ring buffer instances (allocated in Rust, extern here)
 * -------------------------------------------------------------------------- */

/**
 * @brief Ring buffer from MS/TP (Core 1) to BACnet/IP (Core 0).
 *
 * Core 1 writes (producer), Core 0 reads (consumer).
 * Declared in Rust as a static in a .shared_mem linker section.
 */
extern ipc_ring_t mstp_to_ip_ring;

/**
 * @brief Ring buffer from BACnet/IP (Core 0) to MS/TP (Core 1).
 *
 * Core 0 writes (producer), Core 1 reads (consumer).
 */
extern ipc_ring_t ip_to_mstp_ring;

/* --------------------------------------------------------------------------
 * Watchdog heartbeat
 *
 * core1_heartbeat is incremented once per Core 1 main-loop iteration.
 * Core 0 monitors it to detect a stalled Core 1 (see firmware/src/core1.rs).
 * Allocated in Rust (no_mangle static); declared extern here for C access.
 * -------------------------------------------------------------------------- */

/** Heartbeat counter incremented by Core 1 each iteration of its main loop. */
extern volatile uint32_t core1_heartbeat;

/* --------------------------------------------------------------------------
 * ipc_c.c — IPC ring buffer operations
 * -------------------------------------------------------------------------- */

/**
 * @brief Push a PDU onto the ring buffer (producer side).
 * @param ring  Pointer to the ring buffer to write.
 * @param pdu   Pointer to the PDU to copy in.
 * @return true  if the PDU was enqueued successfully.
 * @return false if the ring buffer is full (PDU is dropped).
 */
bool ipc_ring_push(ipc_ring_t *ring, const bacnet_pdu_t *pdu);

/**
 * @brief Pop a PDU from the ring buffer (consumer side).
 * @param ring  Pointer to the ring buffer to read.
 * @param pdu   Output buffer — populated on success.
 * @return true  if a PDU was dequeued.
 * @return false if the ring buffer is empty.
 */
bool ipc_ring_pop(ipc_ring_t *ring, bacnet_pdu_t *pdu);

/**
 * @brief Test whether the ring buffer contains no entries.
 * @param ring  Pointer to the ring buffer.
 * @return true if empty.
 */
bool ipc_ring_is_empty(const ipc_ring_t *ring);

/**
 * @brief Test whether the ring buffer is at capacity.
 * @param ring  Pointer to the ring buffer.
 * @return true if full.
 */
bool ipc_ring_is_full(const ipc_ring_t *ring);

/* --------------------------------------------------------------------------
 * mstp_port.c — UART1 / RS-485 hardware interface
 * -------------------------------------------------------------------------- */

/**
 * @brief Sticky UART receive error flag (L5).
 *
 * Set to true by mstp_port_get_byte() when the PL011 data register indicates
 * a framing, parity, break, or overrun error on the received byte.
 * Callers may read and reset this flag at any point; it is not cleared
 * automatically.  The MS/TP CRC provides the primary error detection;
 * this flag is supplementary diagnostic information.
 */
extern volatile bool g_uart_rx_error;

/**
 * @brief Initialise UART1 and the RS-485 direction pin.
 * @param baud_rate  Desired baud rate (9600, 19200, 38400, or 76800).
 */
void mstp_port_init(uint32_t baud_rate);

/**
 * @brief Auto-detect MS/TP baud rate by listening for valid preamble bytes.
 * Tries 19200, 9600, 38400, 76800 in order, ~2s per rate.
 * @return Detected baud rate, or 19200 if no traffic found.
 */
uint32_t mstp_port_auto_detect_baud(void);

/**
 * @brief Assert or de-assert the RS-485 driver-enable pin (GPIO3).
 * @param transmit  true  → DE high (driver enabled, transmit mode).
 *                  false → DE low  (driver disabled, receive mode).
 */
void mstp_port_set_direction(bool transmit);

/**
 * @brief Check whether a received byte is waiting in the UART FIFO.
 * @return true if at least one byte is available.
 */
bool mstp_port_byte_available(void);

/**
 * @brief Read one byte from the UART1 RX FIFO (non-blocking).
 *
 * Callers must call mstp_port_byte_available() first; behaviour is undefined
 * if the FIFO is empty.
 *
 * @return The received byte.
 */
uint8_t mstp_port_get_byte(void);

/**
 * @brief Write one byte to the UART1 TX FIFO (blocking until space available).
 * @param byte  The byte to transmit.
 */
void mstp_port_put_byte(uint8_t byte);

/**
 * @brief Return the raw microsecond timestamp (lower 32 bits of TIMER_TIMERAWL).
 *
 * Used by silence_timer_ms() / silence_timer_reset() for wrap-safe elapsed
 * time computation.  Rolls over at UINT32_MAX (~71 minutes).
 *
 * @return Microseconds since boot.
 */
uint32_t mstp_port_timer_us(void);

/**
 * @brief Return the current millisecond timestamp.
 *
 * Used by the MS/TP state machine for silence timers and frame timeouts.
 * Rolls over at UINT32_MAX (~49 days).
 *
 * @return Milliseconds since boot.
 */
uint32_t mstp_port_timer_ms(void);

/**
 * @brief Broadcast an unconfirmed Who-Is request on the MS/TP bus.
 *
 * Sends a BACnet Data Not Expecting Reply frame (type 0x05) with an
 * unconfirmed Who-Is APDU to the broadcast address (0xFF).  Increments
 * g_mstp_status.frames_tx.
 *
 * @param src_mac  Source MS/TP MAC address for this node.
 */
void mstp_send_whois(uint8_t src_mac);

/**
 * @brief Non-blocking MS/TP frame receive state machine.
 *
 * Drains the UART1 RX FIFO, advancing the receive state machine one byte
 * at a time.  When a complete valid frame is received, pushes a bacnet_pdu_t
 * onto mstp_to_ip_ring for Core 0 to process.  All BACnet data frames are
 * forwarded (bridge is transparent).
 *
 * Must be called from the Core 1 main loop on every iteration.
 */
void mstp_receive_check(void);

/* --------------------------------------------------------------------------
 * core1_entry.c — Core 1 entry point
 * -------------------------------------------------------------------------- */

/**
 * @brief Core 1 firmware entry point.
 *
 * Called by the Rust multicore::spawn_core1 shim.  Never returns.
 * Initialises the MS/TP port and runs the bridge polling loop indefinitely.
 */
void core1_entry(void);

/* --------------------------------------------------------------------------
 * bacnet_port.c — bacnet-stack platform hooks
 * -------------------------------------------------------------------------- */

/**
 * @brief Initialise the bacnet-stack millisecond timer.
 *
 * No-op on RP2350A — the timer is managed by mstp_port_timer_ms().
 */
void timer_init(void);

/**
 * @brief Return the current millisecond timestamp for bacnet-stack.
 *
 * Delegates to mstp_port_timer_ms().
 *
 * @return Milliseconds since boot.
 */
uint32_t timer_milliseconds(void);

/**
 * @brief Transmit a raw frame over RS-485.
 * @param mstp_port  Opaque pointer to the MS/TP port struct.
 * @param buffer     Frame bytes to transmit.
 * @param nbytes     Number of bytes to transmit.
 */
void RS485_Send_Frame(void *mstp_port, const uint8_t *buffer, uint16_t nbytes);

/**
 * @brief Check UART for received data and feed to MS/TP FSM.
 * @param mstp_port  Opaque pointer to the MS/TP port struct.
 */
void RS485_Check_UART_Data(void *mstp_port);

#ifdef __cplusplus
}
#endif

#endif /* BACNET_BRIDGE_H */
