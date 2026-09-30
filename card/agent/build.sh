#!/usr/bin/env bash
# build.sh [--out DIR]: assemble and link phi-agent (x86-64 assembly, no
# libc, raw system calls), audit it, and place it where the initramfs build
# picks it up. Needs only GNU as and ld: no LLVM, no rustc. With --out, the
# binary goes to DIR and nothing else happens (the host tests build it this
# way). See build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
sources=(agent wire exec xfer stat text errno)

# link OUT: every source assembled, the objects linked static with _start
# as the entry and no symbols kept.
link() {
    local out=$1 obj
    obj=$(mktemp -d)
    for s in "${sources[@]}"; do
        as --64 -I "$here" -o "$obj/$s.o" "$here/$s.S"
    done
    ld -static -nostdlib -e _start -z noexecstack -s -o "$out" "$obj"/*.o
    rm -rf "$obj"
}

if [ $# -gt 0 ]; then
    [ "$1" = "--out" ] && [ $# -eq 2 ] || { echo "usage: build.sh [--out DIR]" >&2; exit 2; }
    mkdir -p "$2"
    link "$2/phi-agent"
    exit 0
fi

. "$here/../../toolchain/env.sh"
OUT="$phi_build/userland/agent"
AUDIT="$phi_root/host/target/debug/phi-isa-audit"
[ -x "$AUDIT" ] || { echo "build.sh: $AUDIT not built (make build)" >&2; exit 1; }
mkdir -p "$OUT"
link "$OUT/phi-agent.new"
file "$OUT/phi-agent.new" | cut -c1-140
echo "== audit (must be clean)"
"$AUDIT" "$OUT/phi-agent.new"
mv "$OUT/phi-agent.new" "$OUT/phi-agent"
ls -l "$OUT/phi-agent"
