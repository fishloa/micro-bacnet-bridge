use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Select the correct memory layout based on the board feature.
    //
    // Exactly one of board-pico / board-pico2 must be enabled; the build
    // will fail with a missing-file error if neither is set.
    let board_pico = env::var("CARGO_FEATURE_BOARD_PICO").is_ok();
    let board_pico2 = env::var("CARGO_FEATURE_BOARD_PICO2").is_ok();

    // c_cpu_flags: CPU/arch flags to pass to arm-none-eabi-gcc.
    //   RP2040 needs -mcpu=cortex-m0plus explicitly (thumbv6m is too generic).
    //   RP2350 uses -march=armv8-m.main+fp (from the target triple); adding
    //   -mcpu=cortex-m33 alongside it triggers a gcc conflict warning, so we
    //   omit -mcpu for RP2350 and let the -march from the triple suffice.
    let (memory_file, c_cpu_flags, c_target) = match (board_pico, board_pico2) {
        (true, false) => (
            "memory-rp2040.x",
            vec!["-mcpu=cortex-m0plus"],
            "thumbv6m-none-eabi",
        ),
        (false, true) => (
            "memory-rp2350.x",
            vec![] as Vec<&str>,
            "thumbv8m.main-none-eabihf",
        ),
        (true, true) => panic!("only one of board-pico / board-pico2 may be enabled at once"),
        (false, false) => panic!("one of board-pico / board-pico2 must be enabled"),
    };

    // Copy the selected memory layout to OUT_DIR so the linker finds it as memory.x
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let memory_x =
        fs::read_to_string(memory_file).unwrap_or_else(|_| panic!("{memory_file} not found"));
    fs::write(out.join("memory.x"), memory_x).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed={memory_file}");
    println!("cargo:rerun-if-changed=memory-rp2040.x");
    println!("cargo:rerun-if-changed=memory-rp2350.x");

    // Linker flags for embedded
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    // link-rp.x is only emitted by embassy-rp for RP2040; RP2350 doesn't need it.
    if board_pico {
        println!("cargo:rustc-link-arg-bins=-Tlink-rp.x");
    }
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");

    // Compile C sources with arm-none-eabi-gcc
    //
    // Source files:
    //   csrc/ipc_c.c        — IPC ring buffer (C side)
    //   csrc/mstp_port.c    — UART1 + RS-485 hardware interface
    //   csrc/bacnet_port.c  — bacnet-stack platform hooks
    //   csrc/core1_entry.c  — Core 1 entry point
    //
    // Include paths:
    //   csrc/               — bacnet_bridge.h (shared types)
    //   lib/bacnet-stack/src — bacnet-stack public headers (bacnet/bacdef.h etc.)
    for src in &[
        "../csrc/ipc_c.c",
        "../csrc/mstp_port.c",
        "../csrc/bacnet_port.c",
        "../csrc/core1_entry.c",
    ] {
        println!("cargo:rerun-if-changed={src}");
    }
    println!("cargo:rerun-if-changed=../csrc/bacnet_bridge.h");

    // RP2350 uses hard-float ABI; RP2040 does not have FPU.
    let mut fpu_flags: Vec<&str> = Vec::new();
    if board_pico2 {
        fpu_flags.push("-mfloat-abi=hard");
        fpu_flags.push("-mfpu=fpv5-sp-d16");
    }

    let mut build = cc::Build::new();
    build
        .compiler("arm-none-eabi-gcc")
        .target(c_target)
        .include("../csrc")
        .include("../lib/bacnet-stack/src")
        .file("../csrc/ipc_c.c")
        .file("../csrc/mstp_port.c")
        .file("../csrc/bacnet_port.c")
        .file("../csrc/core1_entry.c")
        .flag("-mthumb")
        .flag("-Os")
        .flag("-ffreestanding")
        .flag("-nostdlib")
        .flag("-std=c99")
        .flag("-Wall")
        .flag("-Wextra")
        // L4: Enable C warnings so that issues in csrc/ are visible in CI.
        // Previously `.warnings(false)` silenced all diagnostics; now the `-Wall`
        // and `-Wextra` flags above take effect.
        .warnings(true);

    for flag in &c_cpu_flags {
        build.flag(flag);
    }

    for flag in &fpu_flags {
        build.flag(flag);
    }

    build.compile("bacnet");
}
