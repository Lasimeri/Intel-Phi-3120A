#!/usr/bin/env bash
# run.sh: phase P2 exit criterion. Compile hello.c with knc-cc against the
# sysroot (linking the Rust half when build-std.sh has produced it), audit
# the binary for KNC-illegal instructions, run it on the host (same
# instruction subset), and check the exit status. See run.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../env.sh"
root="$phi_root"
here="$root/toolchain/check"
out="$root/toolchain/build/check"
mkdir -p "$out"
CC=knc-cc
AUDIT="$root/host/target/debug/phi-isa-audit"

rs="$here/hello_rs/target/x86_64-knc-linux-musl/release/libhello_rs.a"
extra=""
if [ -f "$rs" ]; then
    # Rust's std on static musl references libunwind (backtraces), built for
    # the card by toolchain/libunwind/build.sh into the sysroot.
    extra="-DHAVE_RUST $rs -lunwind"
    echo "== linking the Rust half ($(rustc --version))"
fi
echo "== compile"
"$CC" -O2 -static -o "$out/hello" "$here/hello.c" $extra -lm -lpthread
echo "== audit (must be clean)"
"$AUDIT" "$out/hello"
echo "== run on host"
"$out/hello"
echo "== objdump: float return path"
"$root/toolchain/build/llvm/bin/llvm-objdump" -d --no-show-raw-insn "$out/hello" | awk '/<scale>:/,/ret/' | head -12
echo "phase P2 check: PASS"
