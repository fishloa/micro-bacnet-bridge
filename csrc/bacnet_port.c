/**
 * @file bacnet_port.c
 * @brief bacnet-stack platform adaptation layer for the RP2350A.
 *
 * bacnet-stack expects certain platform-specific functions to be provided by
 * the port.  This file supplies the minimum set required for the MS/TP master
 * state machine running on Core 1.
 *
 * Functions that are no-ops (because the RP2350A timer is managed entirely
 * through mstp_port.c) are documented as such.  Functions that require
 * bacnet-stack headers are tagged with TODO markers indicating where
 * full integration will occur in a subsequent implementation phase.
 *
 * Include path when compiled by build.rs (cc crate):
 *   -I lib/bacnet-stack/src          (for "bacnet/bacdef.h" etc.)
 *   -I lib/bacnet-stack/ports/...    (for any port-specific headers)
 *
 * @author Icomb Place
 * @copyright SPDX-License-Identifier: MIT
 */

#include <stdint.h>
#include <stdbool.h>

#include "bacnet_bridge.h"
#include "platform_rp2350.h"

/*
 * TODO(phase-4): When bacnet-stack headers are available via the include path
 * configured in build.rs, uncomment the following includes and remove the
 * placeholder forward declarations below.
 *
 *   #include "bacnet/bacdef.h"
 *   #include "bacnet/datalink/mstp.h"
 *   #include "bacnet/datalink/dlmstp.h"
 */

/* --------------------------------------------------------------------------
 * RP2350A UART1 register access
 *
 * Base address comes from platform_rp2350.h.
 * Only the offsets and bit masks specific to this file are defined here.
 * -------------------------------------------------------------------------- */

/** UART flag register offset. */
#define UART_FR_OFFSET  0x018u
/** UART busy bit: set while the UART shift register is transmitting. */
#define UART_FR_BUSY    (1u << 3)
/** Register access macro. */
#define REG(base, off)  (*(volatile uint32_t *)((base) + (off)))

/* --------------------------------------------------------------------------
 * Timer platform hooks
 *
 * bacnet-stack/src/bacnet/basic/sys/mstimer.c (and similar) calls
 * timer_milliseconds() to get the current time.  The function is delegated
 * to mstp_port_timer_ms() which reads the RP2350A hardware timer directly.
 * -------------------------------------------------------------------------- */

/**
 * @brief Initialise the platform millisecond timer.
 *
 * No-op on the RP2350A: the TIMER peripheral runs continuously from power-on
 * and does not require software initialisation.  mstp_port_init() is the
 * correct place for any hardware setup that affects timing.
 */
void timer_init(void)
{
    /* No initialisation required — RP2350A TIMER runs from power-on. */
}

/**
 * @brief Return the current millisecond timestamp.
 *
 * Delegated to mstp_port_timer_ms() which reads TIMER_TIMERAWL directly.
 *
 * @return Milliseconds since boot.
 */
uint32_t timer_milliseconds(void)
{
    return mstp_port_timer_ms();
}

/* --------------------------------------------------------------------------
 * bacnet-stack silence timer callbacks
 *
 * The mstp_port_struct_t has two function-pointer fields used by the MS/TP
 * receive state machine to measure silence on the RS-485 bus:
 *
 *   uint32_t (*SilenceTimer)(void *pArg)   — returns ms since last activity.
 *   void     (*SilenceTimerReset)(void *pArg) — resets the silence counter.
 *
 * These are assigned during MS/TP port initialisation in core1_entry.c.
 * The implementations below are the actual callback targets.
 *
 * TODO(phase-4): Confirm that bacnet-stack's mstp_port_struct_t SilenceTimer
 * field signature matches these prototypes once headers are available.
 * -------------------------------------------------------------------------- */

/** Timestamp (raw microseconds from TIMER_TIMERAWL) of the last RS-485 bus activity.
 *
 * H1: Store raw microseconds, not milliseconds.  mstp_port_timer_ms() divides
 * the 1 MHz counter by 1000, so the quotient wraps every ~71 minutes but the
 * wrap boundary is not aligned to any fixed epoch.  Subtracting two wrapped
 * millisecond values can give a large positive number or even underflow if the
 * timer crossed the 0xFFFFFFFF→0 boundary between the reset and the read.
 *
 * Storing microseconds and converting the *difference* to milliseconds avoids
 * this: (now_us - start_us) uses unsigned 32-bit subtraction which is defined
 * to wrap at 2^32 and always gives the correct positive elapsed time as long
 * as the elapsed interval is < 2^32 µs (~71 minutes), which is guaranteed by
 * the MS/TP timer context (max Tno_token = 500 ms).
 */
static volatile uint32_t silence_timer_start_us = 0u;

/**
 * @brief Return the number of milliseconds since the last RS-485 activity.
 *
 * Called by the MS/TP receive FSM to detect frame-abort and token-loss
 * conditions (Tframe_abort, Tno_token, etc.).
 *
 * @param pArg  Unused context pointer (required by bacnet-stack callback signature).
 * @return Milliseconds elapsed since last call to silence_timer_reset().
 */
uint32_t silence_timer_ms(void *pArg)
{
    uint32_t now_us;
    (void)pArg;

    /* Read the raw 1 MHz counter (microseconds since boot). */
    now_us = mstp_port_timer_us();
    /* Unsigned subtraction + divide: correct across any 32-bit wraparound. */
    return (now_us - silence_timer_start_us) / 1000u;
}

/**
 * @brief Reset the silence timer to the current time.
 *
 * Called by the MS/TP state machine whenever a valid octet is received or
 * a frame transmission completes.
 *
 * @param pArg  Unused context pointer.
 */
void silence_timer_reset(void *pArg)
{
    (void)pArg;
    silence_timer_start_us = mstp_port_timer_us();
}

/* --------------------------------------------------------------------------
 * RS-485 send-frame hook
 *
 * bacnet-stack's MSTP_Master_Node_FSM() calls a platform-supplied
 * RS485_Send_Frame() function to physically transmit a frame.  The
 * implementation below wraps the byte-level mstp_port_* functions.
 *
 * TODO(phase-4): Include "bacnet/datalink/mstp.h" and use the correct
 * mstp_port_struct_t pointer type once headers are on the include path.
 * The current prototype uses void* to avoid a forward-declaration cycle.
 * -------------------------------------------------------------------------- */

/**
 * @brief Transmit a raw MS/TP frame over UART1/RS-485.
 *
 * Asserts DE high (transmit mode), sends each byte, waits for the TX FIFO to
 * drain and the UART to become idle, then de-asserts DE (receive mode).
 *
 * @param mstp_port  Pointer to the MS/TP port struct (cast to void* here;
 *                   will be typed as struct mstp_port_struct_t* in phase 4).
 * @param buffer     Pointer to the frame bytes to transmit.
 * @param nbytes     Number of bytes in the frame.
 */
void RS485_Send_Frame(void *mstp_port, const uint8_t *buffer, uint16_t nbytes)
{
    uint16_t i;

    /* WARNING: Must drain RX FIFO during TX to prevent overflow at 76800 baud.
     * FIFO is only 32 bytes deep = 4.2ms at 76800. RS485_Check_UART_Data must
     * be called between frames; during transmission the RX side is disabled
     * by DE=1 on the SP3485, but late echoes or reflections can still fill it. */

    /* Suppress unused-parameter warning until full integration. */
    (void)mstp_port;

    if ((buffer == (const uint8_t *)0) || (nbytes == 0u)) {
        return;
    }

    /* Assert DE to enable the RS-485 driver. */
    mstp_port_set_direction(true);

    /* Transmit each byte. mstp_port_put_byte() spins until TX FIFO has space. */
    for (i = 0u; i < nbytes; i++) {
        mstp_port_put_byte(buffer[i]);
    }

    /* C4: Wait for the TX shift register to drain completely.
     *
     * After mstp_port_put_byte() returns for the last byte, the byte has
     * entered the TX FIFO but the UART shift register may still be clocking
     * out the previous byte.  De-asserting DE while UART_FR_BUSY is set cuts
     * off the last bits of the frame, corrupting the stop bit seen by all
     * remote nodes.
     *
     * UART_FR_BUSY (bit 3) is cleared only once the shift register is idle
     * and the stop bit has been fully transmitted.
     */
    while (REG(UART1_BASE, UART_FR_OFFSET) & UART_FR_BUSY) {
        /* spin — typically < 1 bit-time at the configured baud rate */
    }

    /* Turnaround delay: 2 bit-times at maximum supported baud (76800) gives
     * ~26 µs headroom for SP3485 propagation delay before we stop driving
     * the bus.  Ten NOPs at 133 MHz = ~75 ns each ≈ 750 ns total, which is
     * conservative but safe.
     */
    {
        volatile uint32_t delay = 10u;
        while (delay--) { __asm__ volatile("nop"); }
    }

    /* Now safe to switch to receive mode. */
    mstp_port_set_direction(false);
}

/**
 * @brief Check the UART1 RX FIFO and update the MS/TP port state.
 *
 * bacnet-stack expects a RS485_Check_UART_Data() function that reads
 * available bytes from the hardware and feeds them into the MS/TP receive
 * state machine via the DataAvailable / DataRegister fields of the port
 * struct, then calls MSTP_Receive_Frame_FSM().
 *
 * TODO(phase-4): Replace void* with struct mstp_port_struct_t* and implement
 * the full byte-feed loop:
 *
 *   while (mstp_port_byte_available()) {
 *       port->DataRegister   = mstp_port_get_byte();
 *       port->DataAvailable  = 1;
 *       MSTP_Receive_Frame_FSM(port);
 *   }
 *
 * @param mstp_port  Pointer to the MS/TP port struct (void* placeholder).
 */
void RS485_Check_UART_Data(void *mstp_port)
{
    /* WARNING: Must drain RX FIFO during TX to prevent overflow at 76800 baud.
     * The PL011 FIFO is only 32 bytes deep = ~4.2 ms at 76800 baud.
     * If this function is not called frequently enough while RS485_Send_Frame
     * is blocking, received bytes will be silently discarded by the UART. */

    /* TODO(phase-4): implement byte-feed loop using typed mstp_port_struct_t. */
    (void)mstp_port;
}
