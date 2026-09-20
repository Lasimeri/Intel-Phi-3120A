#!/usr/bin/env bash
# build.sh: build the knc crate and knc-demo for the card, audit, and leave
# the binary where phi put can reach it. Mirrors card/agent/build.sh: the
# patched LLVM dylib, RUSTC_BOOTSTRAP=1 for the -Z flags a custom target
# needs, a clean build because cargo does not fingerprint the dylib.
#
# card/lib/knc/build.sh must have run first: this links libknc.a out of the
# sysroot. See build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
DYLIB="${PHI_LLVM_DYLIB:-$phi_root/toolchain/build/llvm-dylib}"
[ -e "$DYLIB/lib/libLLVM.so.22.1" ] || { echo "build.sh: patched libLLVM.so.22.1 not found in $DYLIB/lib" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libknc.a" ] || { echo "build.sh: libknc.a not in $PHI_SYSROOT/usr/lib; run card/lib/knc/build.sh first" >&2; exit 1; }
export LD_LIBRARY_PATH="$DYLIB/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export RUSTC_BOOTSTRAP=1
OUT="$phi_build/userland/knc-rs"
AUDIT="$phi_root/host/target/debug/phi-isa-audit"
[ -x "$AUDIT" ] || { echo "build.sh: $AUDIT missing; run 'make build' first" >&2; exit 1; }
mkdir -p "$OUT"
cd "$here"
cargo clean
cargo -Zjson-target-spec build --release
bin="$here/target/x86_64-knc-linux-musl/release/knc-demo"
file "$bin" | cut -c1-140
echo "== audit (must be clean)"
"$AUDIT" "$bin"
cp "$bin" "$OUT/knc-demo"
ls -l "$OUT/knc-demo"
