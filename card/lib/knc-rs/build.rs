//! Points the linker at `libknc.a`, which `card/lib/knc/build.sh` installs
//! into the card sysroot. Nothing is compiled here: the library is
//! generated assembly and building it is that script's job, not cargo's.
//!
//! `PHI_SYSROOT` selects the sysroot, as it does for every other card
//! build; the fallback is the path `toolchain/env.sh` sets by default.
//! See build.md.

use std::path::PathBuf;

fn main() {
    let sysroot = std::env::var("PHI_SYSROOT").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../toolchain/build/sysroot")
    });
    let lib = sysroot.join("usr/lib");

    if !lib.join("libknc.a").exists() {
        panic!(
            "libknc.a not found in {}: run card/lib/knc/build.sh first",
            lib.display()
        );
    }
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=knc");
    println!("cargo:rerun-if-env-changed=PHI_SYSROOT");
    println!("cargo:rerun-if-changed={}", lib.join("libknc.a").display());
}
