#!/usr/bin/env bash
# build.sh [--out DIR]: assemble and link phitop (x86-64 assembly, no
# libc) from its own sources and the ones it shares with phictl
# (../phictl/text.S, sysfs.S, wire.S). The binary goes to
# host/asm/out/phitop (or DIR/phitop); see build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../out"
if [ $# -gt 0 ]; then
    [ "$1" = "--out" ] && [ $# -eq 2 ] || { echo "usage: build.sh [--out DIR]" >&2; exit 2; }
    out=$2
fi
obj="$out/obj-phitop"
mkdir -p "$obj"
for s in "$here"/*.S "$here"/../phictl/text.S "$here"/../phictl/sysfs.S "$here"/../phictl/wire.S; do
    as --64 -I "$here" -I "$here/../phictl" -o "$obj/$(basename "${s%.S}").o" "$s"
done
ld -static -nostdlib -e _start -z noexecstack -o "$out/phitop" "$obj"/*.o
ls -l "$out/phitop"
