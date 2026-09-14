#!/usr/bin/env bash
# build-std.sh: build the Rust half of the P2 check for the card target
# using the distro rustc with the patched LLVM dylib substituted.
# Requires: toolchain/llvm dylib variant built, rust-src package installed,
# musl sysroot and compiler-rt built (the linker is knc-cc). See build-std.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../env.sh"
root="$phi_root"
DYLIB="${PHI_LLVM_DYLIB:-$root/toolchain/build/llvm-dylib}"
if [ ! -e "$DYLIB/lib/libLLVM.so.22.1" ]; then
    echo "build-std.sh: patched libLLVM.so.22.1 not found in $DYLIB/lib; run: PHI_LLVM_VARIANT=dylib toolchain/llvm/build.sh all" >&2
    exit 1
fi
if [ ! -d "$(rustc --print sysroot)/lib/rustlib/src/rust/library" ]; then
    echo "build-std.sh: rust-src missing; run: sudo pacman -S --needed rust-src" >&2
    exit 1
fi
cd "$root/toolchain/check/hello_rs"
export LD_LIBRARY_PATH="$DYLIB/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export RUSTC_BOOTSTRAP=1
echo "== rustc will load: $(ldd "$(rustc --print sysroot)"/lib/librustc_driver-*.so | grep libLLVM)"
# rustc 1.98 gates JSON target specs; RUSTC_BOOTSTRAP=1 (above) makes the stable compiler accept -Z flags.
cargo -Zjson-target-spec build --release
lib="target/x86_64-knc-linux-musl/release/libhello_rs.a"
ls -l "$lib"
echo "== audit"
"$root/host/target/debug/phi-isa-audit" "$lib"
