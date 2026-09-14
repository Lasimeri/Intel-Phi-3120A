#!/usr/bin/env bash
# build.sh: build phi-agent for the card (Rust, static musl, card target),
# audit it, and place it where the initramfs build picks it up. Mirrors
# toolchain/rust/build-std.sh: the patched LLVM dylib, RUSTC_BOOTSTRAP=1 for
# the -Z flags a custom target needs, a clean build because cargo does not
# fingerprint the dylib. See build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../toolchain/env.sh"
DYLIB="${PHI_LLVM_DYLIB:-$phi_root/toolchain/build/llvm-dylib}"
[ -e "$DYLIB/lib/libLLVM.so.22.1" ] || { echo "build.sh: patched libLLVM.so.22.1 not found in $DYLIB/lib" >&2; exit 1; }
export LD_LIBRARY_PATH="$DYLIB/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export RUSTC_BOOTSTRAP=1
OUT="$phi_build/userland/agent"
AUDIT="$phi_root/host/target/debug/phi-isa-audit"
mkdir -p "$OUT"
cd "$here"
cargo clean
cargo -Zjson-target-spec build --release
bin="$here/target/x86_64-knc-linux-musl/release/phi-agent"
file "$bin" | cut -c1-140
echo "== audit (must be clean)"
"$AUDIT" "$bin"
cp "$bin" "$OUT/phi-agent"
ls -l "$OUT/phi-agent"
