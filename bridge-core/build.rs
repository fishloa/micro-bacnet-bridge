use std::env;
use std::path::Path;

fn main() {
    // Only compile the C test harness when running tests on the host.
    // Skip for the embedded ARM target (bridge-core is no_std).
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("thumbv") {
        return;
    }

    let bacnet_src = Path::new("../lib/bacnet-stack/src");
    if !bacnet_src.exists() {
        // Skip if bacnet-stack submodule isn't checked out
        return;
    }

    println!("cargo:rerun-if-changed=csrc/bacnet_test_harness.c");

    // Compile bacnet-stack sources needed for the test harness (host target).
    cc::Build::new()
        .include(bacnet_src)
        .file("csrc/bacnet_test_harness.c")
        .file(bacnet_src.join("bacnet/bacdcode.c"))
        .file(bacnet_src.join("bacnet/bacint.c"))
        .file(bacnet_src.join("bacnet/bacreal.c"))
        .file(bacnet_src.join("bacnet/bacstr.c"))
        .file(bacnet_src.join("bacnet/whois.c"))
        .file(bacnet_src.join("bacnet/iam.c"))
        .file(bacnet_src.join("bacnet/rp.c"))
        .file(bacnet_src.join("bacnet/wp.c"))
        .file(bacnet_src.join("bacnet/proplist.c"))
        .file(bacnet_src.join("bacnet/npdu.c"))
        .file(bacnet_src.join("bacnet/dcc.c"))
        .flag("-std=c99")
        .flag("-DPRINT_ENABLED=0")
        .warnings(false) // bacnet-stack has its own warning policy
        .compile("bacnet_test");
}
