/**
 * @file platform_rp2350.h
 * @brief RP2350A peripheral base addresses and platform constants.
 *
 * All addresses are verified against the rp-pac-7.0.0 crate
 * (src/rp235x/mod.rs) which sources them from the RP2350 datasheet.
 *
 * When porting to a different platform, update this file and
 * firmware/src/platform.rs — all other C sources include this header
 * instead of defining addresses inline.
 *
 * @author Icomb Place
 * @copyright SPDX-License-Identifier: MIT
 */

#ifndef PLATFORM_RP2350_H
#define PLATFORM_RP2350_H

/* --------------------------------------------------------------------------
 * RP2350A peripheral base addresses
 * Source: rp-pac-7.0.0 src/rp235x/mod.rs
 * -------------------------------------------------------------------------- */

/** PSM (Power-on State Machine) register base. */
#define PSM_BASE            0x40018000u

/** Resets register base (peripheral reset control). */
#define RESETS_BASE         0x40020000u

/** IO_BANK0 register base (GPIO pad/function select). */
#define IO_BANK0_BASE       0x40028000u

/** PADS_BANK0 register base (GPIO pad control). */
#define PADS_BANK0_BASE     0x40038000u

/** UART1 register base. */
#define UART1_BASE          0x40078000u

/** TIMER0 register base (1 MHz free-running timer). */
#define TIMER_BASE          0x400b0000u

/** WATCHDOG register base. */
#define WATCHDOG_BASE       0x400d8000u

/** SIO (Single-cycle I/O) register base (GPIO direct control). */
#define SIO_BASE            0xd0000000u

/* --------------------------------------------------------------------------
 * System clock frequency
 *
 * embassy-rp configures the RP2350A system clock to 150 MHz by default.
 * This constant must match the actual PLL configuration used by the
 * Rust firmware (embassy-rp default for RP2350).
 * -------------------------------------------------------------------------- */

/** RP2350A system clock frequency in Hz (embassy-rp default). */
#define SYS_CLK_HZ          150000000u

#endif /* PLATFORM_RP2350_H */
