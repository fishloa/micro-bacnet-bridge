use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // ---- Build version: {major}.{minor}.{build}-pico2 ----
    // Semver: major.minor from Cargo.toml, build number from CI (auto-increments).
    // GITHUB_RUN_NUMBER provides a monotonically increasing integer per workflow.
    // Local builds use 0 as the build number.
    // Example CI output: "0.1.42-pico2"
    let pkg_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".into());
    let build_number = env::var("GITHUB_RUN_NUMBER").unwrap_or_else(|_| "0".into());
    // Extract major.minor from pkg_version (drop the patch digit, replace with build number)
    let parts: Vec<&str> = pkg_version.split('.').collect();
    let major = parts.first().unwrap_or(&"0");
    let minor = parts.get(1).unwrap_or(&"1");
    let full_version = format!("{major}.{minor}.{build_number}-pico2");
    println!("cargo:rustc-env=FIRMWARE_VERSION={full_version}");
    println!("cargo:rustc-env=FIRMWARE_BOARD=pico2");

    // Copy memory layout to OUT_DIR so the linker finds it as memory.x
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let memory_x = fs::read_to_string("memory.x").unwrap_or_else(|_| panic!("memory.x not found"));
    fs::write(out.join("memory.x"), memory_x).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");

    // Linker flags for embedded
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
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

    // Common C compiler flags for ARM Cortex-M33
    let arm_flags: &[&str] = &["-mthumb", "-mfloat-abi=hard", "-mfpu=fpv5-sp-d16", "-Os"];

    // Build bacnet-stack library files (need standard library headers)
    let mut bacnet_build = cc::Build::new();
    bacnet_build
        .compiler("arm-none-eabi-gcc")
        .target("thumbv8m.main-none-eabihf")
        .include("../lib/bacnet-stack/src")
        .file("../lib/bacnet-stack/src/bacnet/datalink/crc.c")
        // Provide minimal C stdlib header stubs (string.h, math.h) since the
        // Homebrew arm-none-eabi-gcc lacks newlib. These are declaration-only —
        // no implementations are linked (bacnet-stack CRC doesn't call them).
        .include("../csrc/newlib-stubs")
        .flag("-ffreestanding")
        .flag("-std=c99")
        .warnings(false); // bacnet-stack has its own warning policy
    for f in arm_flags {
        bacnet_build.flag(f);
    }
    bacnet_build.compile("bacnet_crc");

    // Build our own C sources (freestanding, no standard library)
    let mut build = cc::Build::new();
    build
        .compiler("arm-none-eabi-gcc")
        .target("thumbv8m.main-none-eabihf")
        .include("../csrc")
        .include("../lib/bacnet-stack/src")
        .file("../csrc/ipc_c.c")
        .file("../csrc/mstp_port.c")
        .file("../csrc/bacnet_port.c")
        .file("../csrc/core1_entry.c")
        .flag("-ffreestanding")
        .flag("-nostdlib")
        .flag("-std=c99")
        .flag("-Wall")
        .flag("-Wextra")
        // L4: Enable C warnings so that issues in csrc/ are visible in CI.
        // Previously `.warnings(false)` silenced all diagnostics; now the `-Wall`
        // and `-Wextra` flags above take effect.
        .warnings(true);

    build.compile("bacnet");
}
