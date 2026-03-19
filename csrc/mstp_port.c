/**
 * @file mstp_port.c
 * @brief RP2350A UART1 + SP3485 RS-485 hardware interface for BACnet MS/TP.
 *
 * Provides the low-level byte I/O and timing primitives required by the
 * bacnet-stack MS/TP state machine.  All register accesses are direct (no
 * Pico SDK dependency) so that this file compiles cleanly with
 * arm-none-eabi-gcc under -ffreestanding -nostdlib.
 *
 * Pin assignments (from hardware schematic):
 *   GPIO 3  — SP3485 DE/RE (direction enable, active-high = transmit)
 *   GPIO 4  — UART1 TX
 *   GPIO 5  — UART1 RX
 *
 * Timing-critical functions (byte I/O, direction switching) are placed in
 * the .time_critical section so that the Rust linker script can map them to
 * SRAM for deterministic execution latency, avoiding wait states from XIP
 * flash.
 *
 * @author Icomb Place
 * @copyright SPDX-License-Identifier: MIT
 */

#include <stdint.h>
#include <stdbool.h>

#include "bacnet_bridge.h"
#include "platform_rp2350.h"

/* --------------------------------------------------------------------------
 * UART1 register offsets
 * (PL011 UART, RP2350A datasheet §4.2.8)
 * -------------------------------------------------------------------------- */

#define UART_DR_OFFSET      0x000u   /**< Data register (RX/TX). */
#define UART_FR_OFFSET      0x018u   /**< Flag register. */
#define UART_IBRD_OFFSET    0x024u   /**< Integer baud rate divisor. */
#define UART_FBRD_OFFSET    0x028u   /**< Fractional baud rate divisor. */
#define UART_LCR_H_OFFSET   0x02Cu   /**< Line control register. */
#define UART_CR_OFFSET      0x030u   /**< Control register. */
#define UART_IMSC_OFFSET    0x038u   /**< Interrupt mask set/clear. */
#define UART_ICR_OFFSET     0x044u   /**< Interrupt clear register. */

/* UART_FR bit masks */
#define UART_FR_RXFE        (1u << 4) /**< RX FIFO empty. */
#define UART_FR_TXFF        (1u << 5) /**< TX FIFO full. */
#define UART_FR_BUSY        (1u << 3) /**< UART busy transmitting. */

/* L5: UART_DR error flag bits (bits 8–11 of the data register, PL011 §3.3.1).
 *
 * When any of these bits is set the received byte in bits [7:0] is invalid.
 * The MS/TP CRC in the bacnet-stack state machine provides the primary error
 * detection mechanism — a bad byte will cause a CRC mismatch and the frame
 * will be discarded.  However, detecting hardware errors early lets us track
 * UART health statistics (e.g. for the /api/v1/system/status endpoint) and
 * avoids propagating noise bytes into the state machine unnecessarily.
 *
 * Current implementation: mstp_port_get_byte() records a sticky error flag
 * (g_uart_rx_error) if any error bit is set.  The flag can be read and
 * cleared by bacnet_port.c.  The byte value is still returned — the MS/TP
 * CRC will reject the frame if the data is corrupt.
 */
#define UART_DR_FE          (1u << 8)  /**< Framing error. */
#define UART_DR_PE          (1u << 9)  /**< Parity error. */
#define UART_DR_BE          (1u << 10) /**< Break error. */
#define UART_DR_OE          (1u << 11) /**< Overrun error. */
#define UART_DR_ERROR_MASK  (UART_DR_FE | UART_DR_PE | UART_DR_BE | UART_DR_OE)

/* UART_LCR_H bit masks */
#define UART_LCR_H_FEN      (1u << 4) /**< Enable FIFOs. */
#define UART_LCR_H_WLEN_8   (3u << 5) /**< 8-bit word length. */

/* UART_CR bit masks */
#define UART_CR_UARTEN      (1u << 0)  /**< UART enable. */
#define UART_CR_TXE         (1u << 8)  /**< Transmit enable. */
#define UART_CR_RXE         (1u << 9)  /**< Receive enable. */

/* --------------------------------------------------------------------------
 * IO_BANK0 register offsets
 * Each GPIO has two 4-byte registers: GPIO{n}_STATUS and GPIO{n}_CTRL.
 * -------------------------------------------------------------------------- */

/** Byte offset of GPIO{n}_CTRL within IO_BANK0 (status is at offset*2, ctrl at offset*2+4). */
#define IO_BANK0_GPIO_CTRL(n)   (0x004u + ((uint32_t)(n) * 8u))

/** Function select value for UART in IO_BANK0_GPIO_CTRL. */
#define IO_BANK0_FUNCSEL_UART   2u

/** Function select value for SIO (software-controlled GPIO) in IO_BANK0_GPIO_CTRL. */
#define IO_BANK0_FUNCSEL_SIO    5u

/* --------------------------------------------------------------------------
 * SIO register offsets (GPIO direct control)
 * -------------------------------------------------------------------------- */

#define SIO_GPIO_OUT_SET_OFFSET     0x014u  /**< Atomic set GPIO output. */
#define SIO_GPIO_OUT_CLR_OFFSET     0x018u  /**< Atomic clear GPIO output. */
#define SIO_GPIO_OE_SET_OFFSET      0x024u  /**< Atomic set GPIO output-enable. */

/* --------------------------------------------------------------------------
 * Timer register offsets
 * -------------------------------------------------------------------------- */

/** TIMERAWL — raw lower 32 bits of the 64-bit free-running 1 MHz timer. */
#define TIMER_TIMERAWL_OFFSET   0x028u

/* --------------------------------------------------------------------------
 * RESETS register offsets
 * -------------------------------------------------------------------------- */

#define RESETS_RESET_OFFSET     0x000u  /**< Reset control (1 = held in reset). */
#define RESETS_RESET_DONE_OFFSET 0x008u /**< Reset done status (1 = peripheral released). */

/** Bit position of UART1 in the RESETS register. */
#define RESETS_BIT_UART1        (1u << 23)

/** Bit position of IO_BANK0 in the RESETS register. */
#define RESETS_BIT_IO_BANK0     (1u << 5)

/** Bit position of PADS_BANK0 in the RESETS register. */
#define RESETS_BIT_PADS_BANK0   (1u << 8)

/* --------------------------------------------------------------------------
 * GPIO pin numbers
 * -------------------------------------------------------------------------- */

#define GPIO_RS485_DE   3u   /**< SP3485 driver enable (active high = TX). */
#define GPIO_UART1_TX   4u   /**< UART1 TXD. */
#define GPIO_UART1_RX   5u   /**< UART1 RXD. */

/* --------------------------------------------------------------------------
 * Register access macro
 * -------------------------------------------------------------------------- */

#define REG(base, offset)   (*(volatile uint32_t *)((base) + (offset)))

/* --------------------------------------------------------------------------
 * L5: UART receive error tracking
 * -------------------------------------------------------------------------- */

/**
 * @brief Sticky flag set whenever mstp_port_get_byte() reads a byte with one
 *        or more PL011 error bits set (framing, parity, break, or overrun).
 *
 * Consumers (e.g. bacnet_port.c or the Rust status API) may read and reset
 * this flag at any time.  The MS/TP CRC in the bacnet-stack state machine
 * provides the definitive error detection; this flag is supplementary
 * diagnostic information only.
 */
volatile bool g_uart_rx_error = false;

/* --------------------------------------------------------------------------
 * mstp_port_init
 * -------------------------------------------------------------------------- */

/**
 * @brief Initialise UART1 and the RS-485 direction pin.
 *
 * Sequence:
 *   1. Release UART1, IO_BANK0, and PADS_BANK0 from reset (if held).
 *   2. Configure GPIO3 as a SIO output for DE/RE direction control.
 *   3. Configure GPIO4 and GPIO5 with UART function select.
 *   4. Program baud rate divisors from SYS_CLK_HZ.
 *   5. Enable UART with 8N1, FIFO enabled.
 *   6. Default to receive mode (DE low).
 *
 * @param baud_rate  Desired baud rate in bits per second.
 *                   Supported values: 9600, 19200, 38400, 76800.
 */
__attribute__((section(".time_critical")))
void mstp_port_init(uint32_t baud_rate)
{
    uint32_t brd;
    uint32_t ibrd;
    uint32_t fbrd;

    /* H6: Guard against division by zero and unreasonably large baud rates.
     * Any caller passing 0 or a value above 115200 gets a safe 38400 default. */
    if (baud_rate == 0u || baud_rate > 115200u) {
        baud_rate = 38400u; /* safe default */
    }

    /* -----------------------------------------------------------------------
     * 1. Release peripherals from reset.
     * The Rust embassy-rp initialisation on Core 0 has already released the
     * clocks peripheral and configured the PLL to 133 MHz.  We still ensure
     * IO_BANK0, PADS_BANK0, and UART1 are not held in reset.
     * ---------------------------------------------------------------------- */
    REG(RESETS_BASE, RESETS_RESET_OFFSET) &=
        ~(RESETS_BIT_UART1 | RESETS_BIT_IO_BANK0 | RESETS_BIT_PADS_BANK0);

    /* Wait for the peripherals to come out of reset. */
    while ((REG(RESETS_BASE, RESETS_RESET_DONE_OFFSET) &
            (RESETS_BIT_UART1 | RESETS_BIT_IO_BANK0 | RESETS_BIT_PADS_BANK0)) !=
           (RESETS_BIT_UART1 | RESETS_BIT_IO_BANK0 | RESETS_BIT_PADS_BANK0)) {
        /* Spin. */
    }

    /* -----------------------------------------------------------------------
     * 2. Configure GPIO3 (RS485_DE) as SIO output, initially low (RX mode).
     * ---------------------------------------------------------------------- */
    REG(IO_BANK0_BASE, IO_BANK0_GPIO_CTRL(GPIO_RS485_DE)) = IO_BANK0_FUNCSEL_SIO;
    /* Clear output first, then enable output-enable. */
    REG(SIO_BASE, SIO_GPIO_OUT_CLR_OFFSET) = (1u << GPIO_RS485_DE);
    REG(SIO_BASE, SIO_GPIO_OE_SET_OFFSET)  = (1u << GPIO_RS485_DE);

    /* -----------------------------------------------------------------------
     * 3. Configure GPIO4 (TX) and GPIO5 (RX) for UART1 function.
     * ---------------------------------------------------------------------- */
    REG(IO_BANK0_BASE, IO_BANK0_GPIO_CTRL(GPIO_UART1_TX)) = IO_BANK0_FUNCSEL_UART;
    REG(IO_BANK0_BASE, IO_BANK0_GPIO_CTRL(GPIO_UART1_RX)) = IO_BANK0_FUNCSEL_UART;

    /* -----------------------------------------------------------------------
     * 4. Disable UART before configuring baud rate (PL011 requirement).
     * ---------------------------------------------------------------------- */
    REG(UART1_BASE, UART_CR_OFFSET) = 0u;

    /* Wait for any in-progress transmission to complete. */
    while (REG(UART1_BASE, UART_FR_OFFSET) & UART_FR_BUSY) {
        /* Spin. */
    }

    /* -----------------------------------------------------------------------
     * 5. Compute baud rate divisors.
     *    BRD = SYS_CLK_HZ / (16 * baud_rate)
     *    IBRD = floor(BRD)
     *    FBRD = round(fractional part * 64)
     *
     *    To avoid floating-point in freestanding C, use the identity:
     *    FBRD = (SYS_CLK_HZ * 4 / baud_rate) % 64  (after the integer part).
     *
     *    Full formula: BRD_scaled = SYS_CLK_HZ * 4 / baud_rate
     *    IBRD = BRD_scaled / 64
     *    FBRD = BRD_scaled % 64
     * ---------------------------------------------------------------------- */
    brd  = (SYS_CLK_HZ * 4u) / baud_rate;
    ibrd = brd / 64u;
    fbrd = brd % 64u;

    REG(UART1_BASE, UART_IBRD_OFFSET) = ibrd;
    REG(UART1_BASE, UART_FBRD_OFFSET) = fbrd;

    /* -----------------------------------------------------------------------
     * 6. Set line control: 8 data bits, no parity, 1 stop bit, FIFOs enabled.
     * LCR_H must be written after IBRD/FBRD (PL011 requirement).
     * ---------------------------------------------------------------------- */
    REG(UART1_BASE, UART_LCR_H_OFFSET) = UART_LCR_H_WLEN_8 | UART_LCR_H_FEN;

    /* -----------------------------------------------------------------------
     * 7. Mask all interrupts — we use polled I/O.
     * ---------------------------------------------------------------------- */
    REG(UART1_BASE, UART_IMSC_OFFSET) = 0u;

    /* -----------------------------------------------------------------------
     * 8. Enable UART with TX and RX.
     * ---------------------------------------------------------------------- */
    REG(UART1_BASE, UART_CR_OFFSET) = UART_CR_UARTEN | UART_CR_TXE | UART_CR_RXE;

    /* Default to receive mode. */
    mstp_port_set_direction(false);
}

/* --------------------------------------------------------------------------
 * mstp_port_auto_detect_baud — scan for valid MS/TP frames
 * -------------------------------------------------------------------------- */

/**
 * @brief Auto-detect MS/TP baud rate by listening for valid preamble bytes.
 *
 * Tries each standard baud rate (9600, 19200, 38400, 76800) in order.
 * At each rate, listens for up to ~2 seconds for the MS/TP preamble
 * sequence 0x55 0xFF.  If found, returns that baud rate.
 * If no valid traffic is detected at any rate, returns 19200 as a safe default.
 *
 * @return Detected baud rate, or 19200 if no traffic found.
 */
__attribute__((section(".time_critical")))
uint32_t mstp_port_auto_detect_baud(void)
{
    static const uint32_t rates[] = { 19200u, 9600u, 38400u, 76800u };
    static const uint32_t num_rates = sizeof(rates) / sizeof(rates[0]);

    for (uint32_t r = 0; r < num_rates; r++) {
        mstp_port_init(rates[r]);

        /* Listen for ~2 seconds worth of timer ticks.
         * At 133 MHz, TIMER counts at 1 MHz (1 µs per tick). */
        uint32_t start = mstp_port_timer_us();
        bool got_55 = false;

        while ((mstp_port_timer_us() - start) < 2000000u) {
            if (!mstp_port_byte_available()) {
                continue;
            }
            uint8_t byte = mstp_port_get_byte();

            if (byte == 0x55u) {
                got_55 = true;
            } else if (got_55 && byte == 0xFFu) {
                /* Valid MS/TP preamble found at this baud rate. */
                return rates[r];
            } else {
                got_55 = false;
            }
        }
    }

    /* No traffic detected — return safe default. */
    return 19200u;
}

/* --------------------------------------------------------------------------
 * mstp_port_set_direction
 * -------------------------------------------------------------------------- */

/**
 * @brief Assert or de-assert the RS-485 driver-enable pin (GPIO3).
 *
 * The SP3485 DE and RE pins are tied together on the WIZnet EVB board so a
 * single GPIO controls both transmit (DE=1, RE=0) and receive (DE=0, RE=1)
 * modes.
 *
 * This function is placed in SRAM to minimise direction-switching latency
 * between the last transmitted byte and the return to receive mode.
 *
 * @param transmit  true  → GPIO3 high (transmit / driver enabled).
 *                  false → GPIO3 low  (receive  / driver disabled).
 */
__attribute__((section(".time_critical")))
void mstp_port_set_direction(bool transmit)
{
    if (transmit) {
        REG(SIO_BASE, SIO_GPIO_OUT_SET_OFFSET) = (1u << GPIO_RS485_DE);
    } else {
        REG(SIO_BASE, SIO_GPIO_OUT_CLR_OFFSET) = (1u << GPIO_RS485_DE);
    }
}

/* --------------------------------------------------------------------------
 * mstp_port_byte_available
 * -------------------------------------------------------------------------- */

/**
 * @brief Check whether a received byte is waiting in the UART1 RX FIFO.
 *
 * Reads the UART flag register RXFE bit (bit 4).  When the bit is clear, at
 * least one byte is available.
 *
 * @return true if at least one byte is available.
 */
__attribute__((section(".time_critical")))
bool mstp_port_byte_available(void)
{
    /* RXFE = 1 means the RX FIFO is empty — invert for "available". */
    return (REG(UART1_BASE, UART_FR_OFFSET) & UART_FR_RXFE) == 0u;
}

/* --------------------------------------------------------------------------
 * mstp_port_get_byte
 * -------------------------------------------------------------------------- */

/**
 * @brief Read one byte from the UART1 RX FIFO.
 *
 * The caller must verify data is available with mstp_port_byte_available()
 * before calling this function.
 *
 * L5: The PL011 data register bits [11:8] contain error flags (framing,
 * parity, break, overrun).  When any flag is set the byte value in bits [7:0]
 * may be corrupt.  We record this condition in the sticky g_uart_rx_error flag
 * for diagnostic purposes and still return the byte — the MS/TP CRC in the
 * bacnet-stack state machine provides the primary error detection and will
 * discard frames with bad bytes.
 *
 * Note: clearing the overrun error requires reading the data register (which
 * we do here) per PL011 §3.3.4.  No additional register write is needed.
 *
 * @return The received byte (low 8 bits of DR).
 */
__attribute__((section(".time_critical")))
uint8_t mstp_port_get_byte(void)
{
    uint32_t dr = REG(UART1_BASE, UART_DR_OFFSET);

    /* L5: Check error flag bits [11:8].  Set sticky flag if any are present. */
    if (dr & UART_DR_ERROR_MASK) {
        g_uart_rx_error = true;
    }

    return (uint8_t)(dr & 0xFFu);
}

/* --------------------------------------------------------------------------
 * mstp_port_put_byte
 * -------------------------------------------------------------------------- */

/**
 * @brief Write one byte to the UART1 TX FIFO.
 *
 * Blocks until there is space in the TX FIFO (TXFF clear), then writes the
 * byte.  The FIFO is 32 bytes deep on the PL011; for normal MS/TP frame sizes
 * this spin is very brief.
 *
 * The caller is responsible for:
 *   - Calling mstp_port_set_direction(true) before the first byte of a frame.
 *   - Waiting for the TX FIFO to drain and calling mstp_port_set_direction(false)
 *     after the last byte of a frame (checked via UART_FR_BUSY).
 *
 * @param byte  Byte to transmit.
 */
__attribute__((section(".time_critical")))
void mstp_port_put_byte(uint8_t byte)
{
    /* Wait for TX FIFO to have space. */
    while (REG(UART1_BASE, UART_FR_OFFSET) & UART_FR_TXFF) {
        /* Spin. */
    }

    REG(UART1_BASE, UART_DR_OFFSET) = (uint32_t)byte;
}

/* --------------------------------------------------------------------------
 * mstp_port_timer_ms
 * -------------------------------------------------------------------------- */

/**
 * @brief Return the raw microsecond timestamp from the RP2350A TIMER peripheral.
 *
 * The RP2350A TIMER peripheral contains a 64-bit free-running counter clocked
 * at 1 MHz.  The lower 32 bits (TIMERAWL) give a microsecond count that rolls
 * over after ~71.6 minutes.  This raw value should be used for silence-timer
 * arithmetic (H1 fix): compute elapsed time as
 *   `(now_us - start_us) / 1000u`
 * which correctly handles unsigned 32-bit wraparound.
 *
 * @return Microseconds since boot (wraps at UINT32_MAX ≈ 71 minutes).
 */
__attribute__((section(".time_critical")))
uint32_t mstp_port_timer_us(void)
{
    return REG(TIMER_BASE, TIMER_TIMERAWL_OFFSET);
}

/**
 * @brief Return the current millisecond timestamp.
 *
 * Convenience wrapper over mstp_port_timer_us().  Note that dividing the
 * raw microsecond counter by 1000 before storing a start timestamp loses
 * sub-millisecond precision and creates a wrap-boundary issue at the
 * modulo-1000 boundary — use mstp_port_timer_us() for elapsed-time
 * calculations in the silence timer (see bacnet_port.c).
 *
 * @return Milliseconds since boot (modulo ~71 minutes at microsecond origin).
 */
__attribute__((section(".time_critical")))
uint32_t mstp_port_timer_ms(void)
{
    return mstp_port_timer_us() / 1000u;
}
