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
     *
     * RP2350 pad defaults have IE=0 (input disabled) unlike RP2040.
     * We must explicitly enable input on GPIO5 (RX) and set pull-up
     * (RS-485 idle state is mark/high).
     * ---------------------------------------------------------------------- */
    REG(IO_BANK0_BASE, IO_BANK0_GPIO_CTRL(GPIO_UART1_TX)) = IO_BANK0_FUNCSEL_UART;
    REG(IO_BANK0_BASE, IO_BANK0_GPIO_CTRL(GPIO_UART1_RX)) = IO_BANK0_FUNCSEL_UART;

    /* Configure GPIO5 pad: IE=1, Schmitt=1, PUE=1, PDE=0 */
    /* PADS_BANK0 GPIO5 register = PADS_BANK0_BASE + 0x04 + 5*0x04 = +0x18 */
    /* Value: IE(bit6)=1 | Schmitt(bit1)=1 | PUE(bit3)=1 = 0x4A */
    REG(PADS_BANK0_BASE, 0x04u + GPIO_UART1_RX * 0x04u) = (1u << 6) | (1u << 3) | (1u << 1);

    /* Configure GPIO4 pad: IE=0, OD=0 (output enabled for TX) */
    /* Default should be fine for TX but set explicitly for clarity */
    REG(PADS_BANK0_BASE, 0x04u + GPIO_UART1_TX * 0x04u) = (1u << 1) | (1u << 4); /* Schmitt + 4mA drive */

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
 * mstp_port_loopback_test — PL011 internal loopback self-test
 * -------------------------------------------------------------------------- */

/**
 * @brief Test UART RX by enabling PL011 internal loopback mode.
 *
 * Sends a known byte pattern on TX which is internally routed to RX.
 * No external wiring needed. Returns the number of bytes successfully
 * received (should equal bytes sent if RX works).
 *
 * @param results  If non-NULL, receives up to 8 bytes that were read back.
 * @return Number of bytes successfully looped back (0 = RX broken).
 */
__attribute__((section(".time_critical")))
uint32_t mstp_port_loopback_test(uint8_t *results)
{
    /* Enable loopback: set LBE (bit 7) in CR register */
    uint32_t cr = REG(UART1_BASE, UART_CR_OFFSET);
    REG(UART1_BASE, UART_CR_OFFSET) = cr | (1u << 7);

    /* Drain any stale RX data */
    while (mstp_port_byte_available()) {
        (void)mstp_port_get_byte();
    }

    /* Send and receive 8 test bytes one at a time */
    static const uint8_t pattern[] = { 0x55, 0xFF, 0xAA, 0x01, 0x02, 0x03, 0xDE, 0xAD };
    uint32_t count = 0;

    for (int i = 0; i < 8; i++) {
        /* Send one byte */
        mstp_port_put_byte(pattern[i]);

        /* Wait for TX to complete */
        while (REG(UART1_BASE, UART_FR_OFFSET) & UART_FR_BUSY) {}

        /* Wait for byte to appear in RX FIFO */
        uint32_t timeout = 100000u;
        while (!mstp_port_byte_available() && timeout > 0) { timeout--; }

        if (mstp_port_byte_available()) {
            uint8_t b = mstp_port_get_byte();
            if (results) results[i] = b;
            if (b == pattern[i]) count++;
        } else {
            if (results) results[i] = 0xEE; /* timeout marker */
        }
    }

    /* Disable loopback */
    REG(UART1_BASE, UART_CR_OFFSET) = cr;

    return count;
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

        /* Send a Who-Is at this baud rate to provoke a response from any
         * slave device. Use MAC address from config (or default 1). */
        mstp_send_whois(g_mstp_config.mac_address ? g_mstp_config.mac_address : 1);

        /* Listen for ~2 seconds worth of timer ticks.
         * At 133 MHz, TIMER counts at 1 MHz (1 µs per tick). */
        uint32_t start = mstp_port_timer_us();
        bool got_55 = false;

        while ((mstp_port_timer_us() - start) < 2000000u) {
            core1_check_flash_pause();
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

/* --------------------------------------------------------------------------
 * MS/TP CRC functions (BACnet Annex G)
 * -------------------------------------------------------------------------- */

/* BACnet CRC functions from lib/bacnet-stack/src/bacnet/datalink/crc.c.
 * Linked via build.rs; prototypes declared here. */
extern uint8_t CRC_Calc_Header(uint8_t dataValue, uint8_t crcValue);
extern uint16_t CRC_Calc_Data(uint8_t dataValue, uint16_t crcValue);

/* --------------------------------------------------------------------------
 * mstp_send_frame — transmit a complete MS/TP frame (internal)
 * -------------------------------------------------------------------------- */

/**
 * @brief Transmit a complete MS/TP frame with preamble, header CRC, data, and data CRC.
 *
 * Internal to mstp_port.c — callers use mstp_send_whois() or the outbound
 * PDU path in core1_entry.c which calls RS485_Send_Frame().
 *
 * @param frame_type  MS/TP frame type (e.g., 0x05 = BACnet Data Not Expecting Reply)
 * @param dest        Destination MAC (0xFF = broadcast)
 * @param src         Source MAC (this station)
 * @param data        Payload (NPDU + APDU), or NULL if data_len == 0
 * @param data_len    Payload length in bytes
 */
__attribute__((section(".time_critical")))
static void mstp_send_frame(uint8_t frame_type, uint8_t dest, uint8_t src,
                            const uint8_t *data, uint16_t data_len)
{
    /* Enable transmitter (DE high) */
    mstp_port_set_direction(true);

    /* Preamble */
    mstp_port_put_byte(0x55u);
    mstp_port_put_byte(0xFFu);

    /* Header: frame_type, dest, src, length (big-endian) */
    uint8_t header[5];
    header[0] = frame_type;
    header[1] = dest;
    header[2] = src;
    header[3] = (uint8_t)(data_len >> 8);
    header[4] = (uint8_t)(data_len & 0xFF);

    /* Header CRC (over header bytes) */
    uint8_t hcrc = 0xFFu;
    for (int i = 0; i < 5; i++) {
        mstp_port_put_byte(header[i]);
        hcrc = CRC_Calc_Header(header[i], hcrc);
    }
    mstp_port_put_byte(~hcrc); /* complement */

    /* Data + data CRC (if any) */
    if (data_len > 0 && data != ((void *)0)) {
        uint16_t dcrc = 0xFFFFu;
        for (uint16_t i = 0; i < data_len; i++) {
            mstp_port_put_byte(data[i]);
            dcrc = CRC_Calc_Data(data[i], dcrc);
        }
        dcrc = ~dcrc; /* complement */
        mstp_port_put_byte((uint8_t)(dcrc & 0xFF));
        mstp_port_put_byte((uint8_t)(dcrc >> 8));
    }

    /* Wait for last byte to finish transmitting, then return to receive mode */
    while (REG(UART1_BASE, UART_FR_OFFSET) & UART_FR_BUSY) {
        /* Spin. */
    }
    mstp_port_set_direction(false);
}

/* --------------------------------------------------------------------------
 * mstp_send_whois — broadcast a BACnet Who-Is service request
 * -------------------------------------------------------------------------- */

/**
 * @brief Broadcast an unconfirmed Who-Is request on the MS/TP bus.
 *
 * Frame type 0x05 = BACnet Data Not Expecting Reply (unconfirmed service).
 * NPDU: version=1, control=0x00 (no routing, no expecting reply).
 * APDU: PDU type=0x10 (Unconfirmed-Request), service=0x08 (Who-Is),
 *       no range parameters (discovers all devices).
 *
 * @param src_mac  Source MS/TP MAC address for this node.
 */
__attribute__((section(".time_critical")))
void mstp_send_whois(uint8_t src_mac)
{
    /* NPDU (2 bytes) + APDU (2 bytes) = 4 bytes */
    uint8_t npdu_apdu[4];
    npdu_apdu[0] = 0x01; /* NPDU version */
    npdu_apdu[1] = 0x00; /* NPDU control: no DNET/SNET, no reply expected */
    npdu_apdu[2] = 0x10; /* APDU: Unconfirmed-Request PDU type */
    npdu_apdu[3] = 0x08; /* Service choice: Who-Is */

    /* Flash LED (GPIO25) briefly to indicate Who-Is broadcast */
    REG(SIO_BASE, 0x014u) = (1u << 25); /* GPIO_OUT_SET — LED on */

    /* MS/TP frame type 0x05 = BACnet Data Not Expecting Reply, dest=0xFF (broadcast) */
    mstp_send_frame(0x05, 0xFF, src_mac, npdu_apdu, 4);

    g_mstp_status.frames_tx++;

    /* LED off after transmit */
    REG(SIO_BASE, 0x018u) = (1u << 25); /* GPIO_OUT_CLR — LED off */
}

/* --------------------------------------------------------------------------
 * mstp_poll_for_master — send Poll For Master to a specific MAC
 * -------------------------------------------------------------------------- */

/**
 * @brief Send a Poll For Master frame to a specific MAC address.
 *
 * Frame type 0x01 = Poll For Master (no data).
 * The addressed slave should respond with Reply To Poll For Master (0x02).
 *
 * @param dest_mac  Destination MAC address to poll.
 * @param src_mac   Source MAC (this master station).
 */
__attribute__((section(".time_critical")))
void mstp_poll_for_master(uint8_t dest_mac, uint8_t src_mac)
{
    mstp_send_frame(0x01, dest_mac, src_mac, (void *)0, 0);
    g_mstp_status.frames_tx++;
}

/* --------------------------------------------------------------------------
 * mstp_send_token — pass the token to a specific MAC
 * -------------------------------------------------------------------------- */

/**
 * @brief Send a Token frame to a specific MAC address.
 *
 * Frame type 0x00 = Token (no data).
 * Grants the addressed station permission to transmit.
 *
 * @param dest_mac  Destination MAC to receive the token.
 * @param src_mac   Source MAC (this master station).
 */
__attribute__((section(".time_critical")))
void mstp_send_token(uint8_t dest_mac, uint8_t src_mac)
{
    mstp_send_frame(0x00, dest_mac, src_mac, (void *)0, 0);
    g_mstp_status.frames_tx++;
}

/* --------------------------------------------------------------------------
 * mstp_receive_check — non-blocking MS/TP frame receive state machine
 * -------------------------------------------------------------------------- */

/** Receive state machine states */
#define RX_IDLE         0
#define RX_PREAMBLE2    1
#define RX_HEADER       2
#define RX_HEADER_CRC   3
#define RX_DATA         4
#define RX_DATA_CRC1    5
#define RX_DATA_CRC2    6

static uint8_t rx_state = RX_IDLE;
static uint8_t rx_last_frame_type = 0;
static uint8_t rx_last_src_mac = 0;
static uint8_t rx_header[5];
static uint8_t rx_header_idx;

/* rx_data must hold up to BACNET_PDU_MAX_DATA (501) bytes of payload plus
 * one extra slot used temporarily for the CRC low byte during RX_DATA_CRC1.
 * 512 bytes provides sufficient margin (501 + 1 CRC staging + 10 spare). */
static uint8_t rx_data[512];
static uint16_t rx_data_idx;
static uint16_t rx_data_len;

/**
 * @brief Non-blocking receive: process available UART bytes through the
 *        MS/TP frame receive state machine.
 *
 * When a complete valid frame is received, pushes it to the mstp_to_ip ring
 * buffer for Core 0 to process.  The bridge is TRANSPARENT — all BACnet data
 * frames are forwarded regardless of destination address; the bridge never
 * consumes frames that are addressed to other nodes.
 */
__attribute__((section(".time_critical")))
void mstp_receive_check(void)
{
    while (mstp_port_byte_available()) {
        uint8_t byte = mstp_port_get_byte();

        switch (rx_state) {
        case RX_IDLE:
            if (byte == 0x55u) rx_state = RX_PREAMBLE2;
            break;

        case RX_PREAMBLE2:
            if (byte == 0xFFu) {
                rx_state = RX_HEADER;
                rx_header_idx = 0;
            } else if (byte != 0x55u) {
                rx_state = RX_IDLE;
            }
            /* else: repeated 0x55, stay in PREAMBLE2 */
            break;

        case RX_HEADER:
            rx_header[rx_header_idx++] = byte;
            if (rx_header_idx >= 5) {
                rx_state = RX_HEADER_CRC;
            }
            break;

        case RX_HEADER_CRC: {
            /* Verify header CRC */
            uint8_t crc = 0xFFu;
            for (int i = 0; i < 5; i++) crc = CRC_Calc_Header(rx_header[i], crc);
            crc = CRC_Calc_Header(byte, crc);
            if (crc != 0x55u) { /* valid CRC-8 remainder */
                /* Bad header CRC */
                g_mstp_status.errors_rx++;
                rx_state = RX_IDLE;
                break;
            }
            rx_data_len = ((uint16_t)rx_header[3] << 8) | rx_header[4];
            if (rx_data_len == 0) {
                /* No data — frame complete (Token, Poll For Master, etc.) */
                g_mstp_status.frames_rx++;
                g_mstp_status.bus_active = 1;
                /* Flash LED twice on RX */
                REG(SIO_BASE, 0x014u) = (1u << 25);
                for (volatile int d = 0; d < 50000; d++) {}
                REG(SIO_BASE, 0x018u) = (1u << 25);
                for (volatile int d = 0; d < 30000; d++) {}
                REG(SIO_BASE, 0x014u) = (1u << 25);
                for (volatile int d = 0; d < 50000; d++) {}
                REG(SIO_BASE, 0x018u) = (1u << 25);
                rx_last_frame_type = rx_header[0];
                rx_last_src_mac    = rx_header[2];
                rx_state = RX_IDLE;
            } else if (rx_data_len > BACNET_PDU_MAX_DATA) {
                /* Too large for our PDU — skip */
                rx_state = RX_IDLE;
            } else {
                rx_data_idx = 0;
                rx_state = RX_DATA;
            }
            break;
        }

        case RX_DATA:
            rx_data[rx_data_idx++] = byte;
            if (rx_data_idx >= rx_data_len) {
                rx_state = RX_DATA_CRC1;
            }
            break;

        case RX_DATA_CRC1:
            /* First byte of 16-bit CRC (low byte) — stage temporarily */
            rx_data[rx_data_idx] = byte;
            rx_state = RX_DATA_CRC2;
            break;

        case RX_DATA_CRC2: {
            /* Verify data CRC */
            uint16_t crc = 0xFFFFu;
            for (uint16_t i = 0; i < rx_data_len; i++) {
                crc = CRC_Calc_Data(rx_data[i], crc);
            }
            crc = CRC_Calc_Data(rx_data[rx_data_idx], crc); /* CRC low byte */
            crc = CRC_Calc_Data(byte, crc);                  /* CRC high byte */

            if (crc != 0xF0B8u) { /* valid CRC-16 remainder */
                g_mstp_status.errors_rx++;
                rx_state = RX_IDLE;
                break;
            }

            /* Valid frame received — flash LED twice quickly */
            g_mstp_status.frames_rx++;
            g_mstp_status.bus_active = 1;
            REG(SIO_BASE, 0x014u) = (1u << 25); /* LED on */
            for (volatile int d = 0; d < 50000; d++) {}
            REG(SIO_BASE, 0x018u) = (1u << 25); /* LED off */
            for (volatile int d = 0; d < 30000; d++) {}
            REG(SIO_BASE, 0x014u) = (1u << 25); /* LED on */
            for (volatile int d = 0; d < 50000; d++) {}
            REG(SIO_BASE, 0x018u) = (1u << 25); /* LED off */

            uint8_t frame_type = rx_header[0];
            uint8_t src_mac    = rx_header[2];

            /* Save for mstp_receive_frame_wait() */
            rx_last_frame_type = frame_type;
            rx_last_src_mac    = src_mac;

            /* Forward ALL BACnet data frames to Core 0 (bridge is transparent).
             * Frame type 0x05 = BACnet Data Not Expecting Reply.
             * Frame type 0x06 = BACnet Data Expecting Reply.
             * Core 0 snoops I-Am responses to populate the device list but
             * does NOT consume them — they are still forwarded. */
            if ((frame_type == 0x05 || frame_type == 0x06) && rx_data_len > 0) {
                bacnet_pdu_t pdu;
                pdu.source_net     = 0;
                pdu.source_mac[0]  = src_mac;
                pdu.source_mac_len = 1;
                pdu.dest_net       = 0;
                pdu.dest_mac[0]    = rx_header[1]; /* dest MAC */
                pdu.dest_mac_len   = 1;
                pdu.pdu_type       = PDU_TYPE_MSTP;
                pdu.data_len       = rx_data_len;
                for (uint16_t i = 0; i < rx_data_len; i++) {
                    pdu.data[i] = rx_data[i];
                }
                ipc_ring_push(&mstp_to_ip_ring, &pdu);
            }

            rx_state = RX_IDLE;
            break;
        }

        default:
            rx_state = RX_IDLE;
            break;
        }
    }
}

/* --------------------------------------------------------------------------
 * mstp_receive_frame_wait — blocking receive with timeout
 * -------------------------------------------------------------------------- */

__attribute__((section(".time_critical")))
bool mstp_receive_frame_wait(uint32_t timeout_us, uint8_t *out_type, uint8_t *out_src)
{
    uint32_t start = mstp_port_timer_us();
    uint32_t prev_rx = g_mstp_status.frames_rx;

    while ((mstp_port_timer_us() - start) < timeout_us) {
        core1_check_flash_pause();
        mstp_receive_check();
        if (g_mstp_status.frames_rx != prev_rx) {
            if (out_type) *out_type = rx_last_frame_type;
            if (out_src) *out_src = rx_last_src_mac;
            return true;
        }
    }
    return false;
}
