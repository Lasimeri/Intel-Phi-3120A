#!/usr/bin/env bash
# run.sh: phase P2 exit criterion. Compile hello.c with knc-cc against the
# sysroot, audit the binary for KNC-illegal instructions, run it on the host
# (same instruction subset), and check the exit status. See run.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out="$root/toolchain/build/check"
mkdir -p "$out"
CC="$root/toolchain/clang/knc-cc"
AUDIT="$root/host/target/debug/phi-isa-audit"

rs="$here/hello_rs/target/x86_64-knc-linux-musl/release/libhello_rs.a"
extra=""
if [ -f "$rs" ]; then extra="$rs"; echo "== linking the Rust half"; fi
echo "== compile"
"$CC" -O2 -static -o "$out/hello" "$here/hello.c" $extra -lm -lpthread
echo "== audit (must be clean)"
"$AUDIT" "$out/hello"
echo "== run on host"
"$out/hello"
echo "== objdump: float return path"
"$root/toolchain/build/llvm/bin/llvm-objdump" -d --no-show-raw-insn "$out/hello" | awk '/<scale>:/,/ret/' | head -12
echo "phase P2 check: PASS"
