use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Copy memory.x to OUT_DIR for the linker
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let memory_x = fs::read_to_string("memory.x").expect("memory.x not found");
    fs::write(out.join("memory.x"), memory_x).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");

    // Linker flags for embedded
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tlink-rp.x");
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

    cc::Build::new()
        .compiler("arm-none-eabi-gcc")
        .target("thumbv6m-none-eabi")
        .include("../csrc")
        .include("../lib/bacnet-stack/src")
        .file("../csrc/ipc_c.c")
        .file("../csrc/mstp_port.c")
        .file("../csrc/bacnet_port.c")
        .file("../csrc/core1_entry.c")
        .flag("-mcpu=cortex-m0plus")
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
        .warnings(true)
        .compile("bacnet");
}
