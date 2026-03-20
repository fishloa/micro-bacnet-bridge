/**
 * @file core1_entry.c
 * @brief Core 1 firmware entry point for the BACnet MS/TP state machine.
 *
 * This translation unit provides the C-side entry point for RP2350A Core 1.
 * The Rust runtime on Core 0 launches Core 1 via embassy-rp's
 * multicore::spawn_core1(), which calls core1_entry() with the stack pointer
 * already configured.
 *
 * Responsibilities:
 *   - Initialise the UART1 / RS-485 hardware (mstp_port_init).
 *   - Set up the bacnet-stack MS/TP port structure with buffers and callbacks.
 *   - Run the MS/TP receive and master-node state machines in a tight loop.
 *   - Forward received BACnet PDUs to Core 0 via mstp_to_ip_ring.
 *   - Dequeue outbound PDUs from ip_to_mstp_ring and transmit them over RS-485.
 *
 * All state is statically allocated (no malloc/free).  The MS/TP port struct
 * and I/O buffers are file-scope so that their addresses are stable across
 * the lifetime of the firmware.
 *
 * @author Icomb Place
 * @copyright SPDX-License-Identifier: MIT
 */

#include <stdint.h>
#include <stdbool.h>

#include "bacnet_bridge.h"
/* platform_rp2350.h is already included transitively via bacnet_bridge.h */

/*
 * TODO(phase-4): Uncomment once bacnet-stack include paths are confirmed in
 * build.rs and the submodule headers are available:
 *
 *   #include "bacnet/bacdef.h"
 *   #include "bacnet/datalink/mstp.h"
 *   #include "bacnet/datalink/mstpdef.h"
 *   #include "bacnet/datalink/dlmstp.h"
 *   #include "bacnet/npdu.h"
 */

/* --------------------------------------------------------------------------
 * Forward declarations for bacnet_port.c callbacks
 * These will be typed correctly in phase 4 once the mstp_port_struct_t type
 * is available from bacnet-stack headers.
 * -------------------------------------------------------------------------- */

extern uint32_t silence_timer_ms(void *pArg);
extern void     silence_timer_reset(void *pArg);

/* --------------------------------------------------------------------------
 * MS/TP configuration defaults
 *
 * These values will be read from flash config in a later phase.  Hardcoded
 * defaults are used for initial bring-up.
 * -------------------------------------------------------------------------- */

/** Default MS/TP MAC address for this node (valid range: 0–127 for masters). */
#define MSTP_DEFAULT_MAC_ADDRESS    2u

/** Shared config written by Rust before Core 1 launch. */
volatile mstp_config_t g_mstp_config;

/** Shared status written by Core 1, read by Core 0. */
volatile mstp_status_t g_mstp_status;

/** Flash pause handshake — Core 0 sets request, Core 1 acks with paused. */
volatile uint8_t g_flash_pause_request = 0;
volatile uint8_t g_core1_paused = 0;

/** Maximum master address to poll (0–127). */
#define MSTP_DEFAULT_MAX_MASTER     127u

/** Maximum info frames per token hold. */
#define MSTP_DEFAULT_MAX_INFO_FRAMES 1u

/* --------------------------------------------------------------------------
 * Static I/O buffers for the MS/TP port struct
 *
 * InputBuffer  — holds bytes as they arrive from UART1 during frame reception.
 * OutputBuffer — holds the frame being assembled for transmission.
 *
 * MSTP_FRAME_NPDU_MAX is 501 bytes per the BACnet standard.  We use 502 to
 * accommodate the maximum data length plus one byte of headroom.
 *
 * TODO(phase-4): Replace the literal 502 with MSTP_FRAME_NPDU_MAX + 1 once
 * the bacnet-stack header is included.
 * -------------------------------------------------------------------------- */

#define MSTP_INPUT_BUFFER_SIZE  502u
#define MSTP_OUTPUT_BUFFER_SIZE 502u

static uint8_t mstp_input_buf[MSTP_INPUT_BUFFER_SIZE];
static uint8_t mstp_output_buf[MSTP_OUTPUT_BUFFER_SIZE];

/* --------------------------------------------------------------------------
 * MS/TP port struct placeholder
 *
 * In phase 4 this will be:
 *   static struct mstp_port_struct_t mstp_port;
 *
 * For now it is an opaque byte array sized to accommodate the struct, so the
 * file compiles without the bacnet-stack headers.  The actual struct size on
 * RP2350A/Cortex-M33 (4-byte pointers) is approximately 300 bytes; 512 bytes
 * provides a comfortable margin.
 *
 * TODO(phase-4): Replace with:
 *   static struct mstp_port_struct_t mstp_port;
 * -------------------------------------------------------------------------- */

#define MSTP_PORT_STRUCT_OPAQUE_SIZE 512u
/* aligned(4): guarantees struct mstp_port_struct_t (M5) pointer alignment. */
static uint8_t mstp_port_opaque[MSTP_PORT_STRUCT_OPAQUE_SIZE] __attribute__((aligned(4)));

/* --------------------------------------------------------------------------
 * Static PDU scratch buffer for IPC operations
 * -------------------------------------------------------------------------- */

/** Scratch buffer used to hold a dequeued PDU before forwarding to RS-485. */
static bacnet_pdu_t outbound_pdu;

/* --------------------------------------------------------------------------
 * Who-Is broadcast timer
 *
 * The bridge broadcasts a Who-Is every WHOIS_INTERVAL_US on the MS/TP bus to
 * discover (and re-discover) devices.  The interval is 10 seconds.
 * -------------------------------------------------------------------------- */

/** Who-Is broadcast interval in microseconds (10 seconds). */
#define WHOIS_INTERVAL_US   10000000u

/** Poll For Master interval — scan one MAC every 200ms. */
#define POLL_INTERVAL_US    200000u

/** Timeout waiting for Reply To Poll For Master (100ms per BACnet standard). */
#define POLL_REPLY_TIMEOUT_US  100000u

/** Maximum MAC address to poll (per BACnet, master MACs are 0–127). */
#define MAX_POLL_MAC        127u

/* --------------------------------------------------------------------------
 * mstp_poll — MS/TP state machine invocation
 * -------------------------------------------------------------------------- */

/**
 * @brief MS/TP poll: receive any incoming frames.
 *
 * Delegates to mstp_receive_check() in mstp_port.c which drives the
 * frame-level receive state machine and pushes complete frames onto
 * mstp_to_ip_ring for Core 0.
 *
 * TODO(phase-4): Replace the body of this function with:
 *
 *   struct mstp_port_struct_t *port = (struct mstp_port_struct_t *)mstp_port_opaque;
 *
 *   // Feed any received bytes into the receive FSM.
 *   RS485_Check_UART_Data(port);
 *
 *   // Drive the receive frame FSM.
 *   MSTP_Receive_Frame_FSM(port);
 *
 *   // Drive the master-node FSM; returns true if a PDU was received.
 *   if (MSTP_Master_Node_FSM(port)) {
 *       // A complete BACnet PDU has been received — push it to Core 0.
 *       mstp_handle_received_frame(port);
 *   }
 */
__attribute__((section(".time_critical")))
static void mstp_poll(void)
{
    mstp_receive_check();
}

/* --------------------------------------------------------------------------
 * mstp_handle_received_frame — push received PDU to Core 0
 *
 * TODO(phase-4): Implement this function once the mstp_port_struct_t type is
 * available.  It should:
 *   1. Read SourceAddress, DestinationAddress, FrameType, DataLength from port.
 *   2. Populate a bacnet_pdu_t from the MS/TP port InputBuffer.
 *   3. Call ipc_ring_push(&mstp_to_ip_ring, &pdu).
 *   4. If the ring is full, increment a dropped-frame counter (add to system
 *      status struct in a later phase).
 * -------------------------------------------------------------------------- */

/* --------------------------------------------------------------------------
 * mstp_transmit_outbound — drain ip_to_mstp_ring and send frames
 *
 * Checks ip_to_mstp_ring for a queued outbound PDU.  If one is available it
 * is dequeued and forwarded to RS485_Send_Frame().
 *
 * TODO(phase-4): Wrap the data in a proper MS/TP frame header (preamble, type,
 * dest/src addresses, length, CRC) using MSTP_Create_Frame() before sending.
 * -------------------------------------------------------------------------- */

/**
 * @brief Dequeue one outbound PDU from ip_to_mstp_ring and transmit it.
 *
 * Placed in .time_critical so it executes from SRAM, guarding against XIP
 * stalls during flash operations on Core 0 (see C3 in the resilience audit).
 */
__attribute__((section(".time_critical")))
static void mstp_transmit_outbound(void)
{
    if (!ipc_ring_pop(&ip_to_mstp_ring, &outbound_pdu)) {
        /* Nothing pending — return immediately. */
        return;
    }

    if (outbound_pdu.data_len == 0u || outbound_pdu.data_len > BACNET_PDU_MAX_DATA) {
        /* Malformed entry — discard silently. */
        return;
    }

    /*
     * TODO(phase-4): Instead of sending raw NPDU bytes, wrap in an MS/TP
     * frame using the bacnet-stack helper:
     *
     *   uint16_t frame_len = MSTP_Create_Frame(
     *       mstp_output_buf,
     *       MSTP_OUTPUT_BUFFER_SIZE,
     *       FRAME_TYPE_BACNET_DATA_NOT_EXPECTING_REPLY,  // or _EXPECTING_REPLY
     *       outbound_pdu.dest_mac[0],                    // destination MAC
     *       mstp_port->This_Station,                     // source MAC
     *       outbound_pdu.data,
     *       outbound_pdu.data_len);
     *
     *   RS485_Send_Frame(mstp_port, mstp_output_buf, frame_len);
     *
     * For now, send the raw NPDU bytes directly for loopback testing only.
     */

    /* Suppress unused-variable warning on output buffer during placeholder phase. */
    (void)mstp_output_buf;

    RS485_Send_Frame(
        (void *)mstp_port_opaque,
        outbound_pdu.data,
        outbound_pdu.data_len);
}

/* --------------------------------------------------------------------------
 * core1_entry — Core 1 firmware entry point
 * -------------------------------------------------------------------------- */

/**
 * @brief Core 1 firmware entry point.
 *
 * Called by the Rust multicore::spawn_core1() shim after Core 1's stack and
 * vector table are configured.  This function never returns.
 *
 * Placed in .time_critical so it (and the functions it calls) run from SRAM,
 * not XIP flash.  This prevents XIP cache stalls when Core 0 erases or writes
 * flash (e.g. saving config), which would otherwise pause Core 1 mid-frame
 * and violate MS/TP timing constraints (C3 in resilience audit).
 *
 * Flash-pause protocol: Core 0 sets g_flash_pause_request before any
 * config-sector erase/write. Core 1 detects this in core1_check_flash_pause()
 * and spins in SRAM (echoing SIO FIFO tokens) until the flag is cleared.
 * OTA staging writes target the upper flash area only and rely on embassy-rp's
 * built-in multicore safety; no explicit pause is needed for those.
 *
 * Initialisation sequence:
 *   1. Zero the MS/TP port opaque struct.
 *   2. Initialise UART1 and RS-485 GPIO.
 *   3. Set up MS/TP port struct fields (buffers, callbacks, MAC address).
 *   4. Call MSTP_Init() to reset the state machines.
 *   5. Enter the polling loop.
 *
 * Main loop:
 *   - Increment core1_heartbeat so Core 0 can detect a stalled Core 1 (C2).
 *   - Call mstp_poll() to advance the MS/TP receive and master-node FSMs.
 *   - Call mstp_transmit_outbound() to forward any queued BACnet/IP PDUs.
 *   - Broadcast Who-Is every WHOIS_INTERVAL_US (10 s) to discover devices.
 *   - Any received MS/TP PDU is pushed to mstp_to_ip_ring inside mstp_poll().
 */
__attribute__((section(".time_critical")))
void core1_entry(void)
{
    /* -----------------------------------------------------------------------
     * Step 1: Zero-initialise the opaque MS/TP port struct.
     * ---------------------------------------------------------------------- */
    /* Zero-initialise without libc memset. */
    {
        uint8_t *p = (uint8_t *)mstp_port_opaque;
        for (uint32_t i = 0; i < sizeof(mstp_port_opaque); i++) {
            p[i] = 0;
        }
    }

    /* -----------------------------------------------------------------------
     * Step 2: Initialise UART1 and RS-485 direction GPIO.
     * ---------------------------------------------------------------------- */
    {
        uint32_t baud = g_mstp_config.baud_rate;
        if (baud == 0u) {
            g_mstp_status.detecting = 1;
            baud = mstp_port_auto_detect_baud();
            g_mstp_status.detecting = 0;
        }
        mstp_port_init(baud);
        g_mstp_status.active_baud = baud;
        g_mstp_status.parity = 0; /* 8N1 always */
    }

    /* -----------------------------------------------------------------------
     * Step 3: Populate MS/TP port struct fields.
     *
     * TODO(phase-4): Cast mstp_port_opaque to struct mstp_port_struct_t* and
     * assign the following fields:
     *
     *   port->InputBuffer         = mstp_input_buf;
     *   port->InputBufferSize     = MSTP_INPUT_BUFFER_SIZE;
     *   port->OutputBuffer        = mstp_output_buf;
     *   port->OutputBufferSize    = MSTP_OUTPUT_BUFFER_SIZE;
     *   port->This_Station        = MSTP_DEFAULT_MAC_ADDRESS;
     *   port->Nmax_master         = MSTP_DEFAULT_MAX_MASTER;
     *   port->Nmax_info_frames    = MSTP_DEFAULT_MAX_INFO_FRAMES;
     *   port->SilenceTimer        = silence_timer_ms;
     *   port->SilenceTimerReset   = silence_timer_reset;
     *
     * Suppress unused-variable warnings for the buffers during this phase.
     * ---------------------------------------------------------------------- */
    (void)mstp_input_buf;
    (void)mstp_output_buf;
    (void)silence_timer_ms;
    (void)silence_timer_reset;

    /* -----------------------------------------------------------------------
     * Step 4: Initialise the bacnet-stack MS/TP state machine.
     *
     * TODO(phase-4):
     *   MSTP_Init((struct mstp_port_struct_t *)mstp_port_opaque);
     * ---------------------------------------------------------------------- */

    /* -----------------------------------------------------------------------
     * Step 5: Polling loop — runs forever on Core 1.
     * ---------------------------------------------------------------------- */

    /* Track whether auto-detect mode is active (baud_rate == 0 in config). */
    uint8_t auto_detect_mode = (g_mstp_config.baud_rate == 0u);
    /* Timestamp of last valid MS/TP frame — for re-scan trigger. */
    uint32_t last_frame_us = mstp_port_timer_us();
    /* Poll For Master state — scans one MAC per cycle. */
    uint8_t poll_next_mac = 0;
    uint32_t last_poll_us = mstp_port_timer_us();
    /* Timestamp of last Who-Is broadcast. */
    uint32_t last_whois_us = mstp_port_timer_us();

    for (;;) {
        core1_check_flash_pause();

        /* Increment the watchdog heartbeat counter (C2). */
        core1_heartbeat++;

        /* In auto-detect mode, re-scan if bus has been silent for 60 seconds.
         * Longer interval gives Poll For Master time to discover slaves. */
        if (auto_detect_mode && !g_mstp_status.bus_active) {
            uint32_t elapsed = mstp_port_timer_us() - last_frame_us;
            if (elapsed > 60000000u) { /* 60 seconds */
                g_mstp_status.detecting = 1;
                uint32_t new_baud = mstp_port_auto_detect_baud();
                g_mstp_status.detecting = 0;
                if (new_baud != g_mstp_status.active_baud) {
                    mstp_port_init(new_baud);
                    g_mstp_status.active_baud = new_baud;
                }
                last_frame_us = mstp_port_timer_us();
            }
        }

        /* Advance the MS/TP receive and master-node state machines. */
        mstp_poll();

        /* Forward any queued outbound PDUs from Core 0 to RS-485. */
        mstp_transmit_outbound();

        /* Broadcast Who-Is every WHOIS_INTERVAL_US to discover devices. */
        {
            uint32_t now_us = mstp_port_timer_us();
            if ((now_us - last_whois_us) >= WHOIS_INTERVAL_US) {
                mstp_send_whois(g_mstp_config.mac_address);
                last_whois_us = now_us;
            }
        }

        /* Token grant — cycle through MAC addresses to let slaves transmit.
         *
         * MS/TP slaves do NOT respond to Poll For Master (that's master-only).
         * Slaves can only transmit after receiving a Token (frame type 0x00).
         * After we broadcast Who-Is, slaves queue an I-Am response but can't
         * send it until granted the Token.
         *
         * We send Token to one MAC every POLL_INTERVAL_US, then listen for
         * a response. If the slave has queued data, it transmits immediately. */
        {
            uint32_t now_us = mstp_port_timer_us();
            if ((now_us - last_poll_us) >= POLL_INTERVAL_US) {
                last_poll_us = now_us;

                /* Skip our own MAC */
                if (poll_next_mac == g_mstp_config.mac_address) {
                    poll_next_mac++;
                    if (poll_next_mac > MAX_POLL_MAC) poll_next_mac = 0;
                }

                /* Send Token directly to this MAC — slave can transmit if it has data */
                mstp_send_token(poll_next_mac, g_mstp_config.mac_address);

                /* Wait for the slave to respond with data */
                mstp_receive_frame_wait(POLL_REPLY_TIMEOUT_US, (void *)0, (void *)0);

                /* Advance to next MAC */
                poll_next_mac++;
                if (poll_next_mac > MAX_POLL_MAC) poll_next_mac = 0;
            }
        }

        /*
         * NOTE: No sleep or yield here.  Core 1 is dedicated to real-time
         * MS/TP processing and must respond to UART bytes within the bit-time
         * of the configured baud rate.  At 9600 baud, one bit time is ~104 µs.
         * At 76800 baud, ~13 µs.  A tight spin loop is intentional.
         *
         * If power consumption becomes a concern, a WFI (wait-for-interrupt)
         * can be inserted here provided the UART RX interrupt wakes Core 1,
         * but this is outside the initial scope.
         */
    }
}
