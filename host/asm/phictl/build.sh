#!/usr/bin/env bash
# build.sh [--out DIR]: assemble and link phictl (x86-64 assembly, no
# libc). GNU as and ld are all it needs. The binary goes to
# host/asm/out/phictl (or DIR/phictl); see build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../out"
if [ $# -gt 0 ]; then
    [ "$1" = "--out" ] && [ $# -eq 2 ] || { echo "usage: build.sh [--out DIR]" >&2; exit 2; }
    out=$2
fi
obj="$out/obj"
mkdir -p "$obj"
for s in "$here"/*.S; do
    as --64 -I "$here" -o "$obj/$(basename "${s%.S}").o" "$s"
done
ld -static -nostdlib -e _start -z noexecstack -o "$out/phictl" "$obj"/*.o
ls -l "$out/phictl"
