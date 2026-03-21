use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // ---- Firmware version (Semantic Versioning 2.0.0) ----
    //
    // Single source of truth: firmware/Cargo.toml `version` field.
    // Format: MAJOR.MINOR.PATCH+pico2
    //
    // MAJOR — incompatible API/protocol changes
    // MINOR — new functionality, backwards compatible
    // PATCH — bug fixes, backwards compatible
    // +pico2 — build metadata (board identifier, ignored for precedence)
    //
    // To release a new version: bump `version` in Cargo.toml, commit, tag.
    //
    // Enforcement: if firmware or bridge-core source has changed since the
    // git tag `v{version}`, the version is auto-bumped (patch increment).
    // This ensures every deployed binary has a unique version.
    // Set SKIP_VERSION_CHECK=1 to bypass during active development.
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut version_set = false;

    // Version enforcement — reject builds where source changed but version didn't.
    if env::var("SKIP_VERSION_CHECK").is_err() {
        let tag = format!("v{version}");
        // Check if the tag exists
        let tag_exists = std::process::Command::new("git")
            .args(["rev-parse", "--verify", &format!("refs/tags/{tag}")])
            .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if tag_exists {
            // Tag exists — check if firmware/, bridge-core/, or csrc/ changed since
            // that tag.  Includes both committed AND uncommitted changes so that
            // a developer can't build dirty source under an old version number.
            let repo_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
                .parent()
                .unwrap()
                .to_path_buf();

            // Committed changes: tag..HEAD
            let committed = std::process::Command::new("git")
                .args([
                    "diff",
                    "--name-only",
                    &tag,
                    "HEAD",
                    "--",
                    "firmware/",
                    "bridge-core/",
                    "csrc/",
                ])
                .current_dir(&repo_root)
                .output()
                .ok();

            // Uncommitted changes: working tree vs HEAD (exclude Cargo.toml
            // to avoid infinite rebuild cycle when build.rs auto-bumps the version)
            let uncommitted = std::process::Command::new("git")
                .args([
                    "diff",
                    "--name-only",
                    "HEAD",
                    "--",
                    "firmware/src/",
                    "bridge-core/src/",
                    "csrc/",
                ])
                .current_dir(&repo_root)
                .output()
                .ok();

            let mut all_changed = String::new();
            if let Some(o) = &committed {
                all_changed.push_str(&String::from_utf8_lossy(&o.stdout));
            }
            if let Some(o) = &uncommitted {
                all_changed.push_str(&String::from_utf8_lossy(&o.stdout));
            }
            let changed = all_changed.trim();

            if !changed.is_empty() {
                let next = suggest_next_patch(&version);
                // Auto-bump: rewrite firmware/Cargo.toml with the next patch version.
                let cargo_toml_path =
                    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml");
                let content = fs::read_to_string(&cargo_toml_path)
                    .expect("failed to read firmware/Cargo.toml");
                let bumped = content.replacen(
                    &format!("version = \"{version}\""),
                    &format!("version = \"{next}\""),
                    1,
                );
                fs::write(&cargo_toml_path, bumped).expect("failed to write firmware/Cargo.toml");

                println!(
                    "cargo:warning=Auto-bumped firmware version: {version} → {next} \
                     (source changed since tag {tag})"
                );

                // Update the version variable for this build.
                let full_version = format!("{next}+pico2");
                println!("cargo:rustc-env=FIRMWARE_VERSION={full_version}");
                // Don't set it again below.
                println!("cargo:rustc-env=FIRMWARE_BOARD=pico2");
                version_set = true;
            }
        }
        // If tag doesn't exist, this is a new version — allow it.
    }

    if !version_set {
        let full_version = format!("{version}+pico2");
        println!("cargo:rustc-env=FIRMWARE_VERSION={full_version}");
        println!("cargo:rustc-env=FIRMWARE_BOARD=pico2");
    }

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
    // Includes CRC functions and the MS/TP state machine (MSTP_Master_Node_FSM,
    // MSTP_Receive_Frame_FSM, MSTP_Create_Frame etc.)
    let mut bacnet_build = cc::Build::new();
    bacnet_build
        .compiler("arm-none-eabi-gcc")
        .target("thumbv8m.main-none-eabihf")
        .include("../lib/bacnet-stack/src")
        .file("../lib/bacnet-stack/src/bacnet/datalink/crc.c")
        .file("../lib/bacnet-stack/src/bacnet/datalink/mstp.c")
        .file("../lib/bacnet-stack/src/bacnet/datalink/cobs.c")
        // Provide minimal C stdlib header stubs (string.h, math.h) since the
        // Homebrew arm-none-eabi-gcc lacks newlib. These are declaration-only —
        // no implementations are linked (bacnet-stack CRC doesn't call them).
        .include("../csrc/newlib-stubs")
        .flag("-ffreestanding")
        .flag("-std=c99")
        .flag("-DPRINT_ENABLED=0")
        .flag("-DMSTP_PDU_PACKET_COUNT=0") // We provide our own MSTP_Get_Send/Put_Receive
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
        .include("../csrc/newlib-stubs")
        .file("../csrc/ipc_c.c")
        .file("../csrc/mstp_port.c")
        .file("../csrc/bacnet_port.c")
        .file("../csrc/core1_entry.c")
        .flag("-ffreestanding")
        .flag("-nostdlib")
        .flag("-std=c99")
        .flag("-DPRINT_ENABLED=0")
        .flag("-Wall")
        .flag("-Wextra")
        // L4: Enable C warnings so that issues in csrc/ are visible in CI.
        // Previously `.warnings(false)` silenced all diagnostics; now the `-Wall`
        // and `-Wextra` flags above take effect.
        .warnings(true);

    build.compile("bacnet");
}

/// Suggest the next patch version: "0.2.0" → "0.2.1".
fn suggest_next_patch(version: &str) -> String {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() == 3 {
        let patch: u32 = parts[2].parse().unwrap_or(0);
        format!("{}.{}.{}", parts[0], parts[1], patch + 1)
    } else {
        format!("{}.0.1", version)
    }
}
