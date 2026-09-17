#!/usr/bin/env bash
# zlib.sh: build zlib for the card (static) into the sysroot. Needed by
# CPython's zlib module and by anything else that compresses. See zlib.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
VER="${PHI_ZLIB_VERSION:-1.3.1}"
URL="https://github.com/madler/zlib/releases/download/v$VER/zlib-$VER.tar.gz"
SRC="$root/card/userland/build/zlib-$VER"
AUDIT="$root/host/target/debug/phi-isa-audit"
[ -x "$AUDIT" ] || { echo "zlib.sh: $AUDIT missing; run 'make build' first" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libc.a" ] || { echo "zlib.sh: no musl sysroot at $PHI_SYSROOT; run toolchain/musl/build.sh first" >&2; exit 1; }
# Pinned download (toolchain/fetch.sh, toolchain/SHA256SUMS).
phi_fetch "zlib-$VER.tar.gz" "$URL"
tarball="$phi_fetched"
rm -rf "$SRC"; mkdir -p "$SRC"
tar -xzf "$tarball" -C "$SRC" --strip-components=1
cd "$SRC"
CC=knc-cc AR=llvm-ar RANLIB=llvm-ranlib ./configure --static --prefix=/usr > configure.log 2>&1
make -j"$(nproc)" > make.log 2>&1
make install DESTDIR="$PHI_SYSROOT" > install.log 2>&1
ls -l "$PHI_SYSROOT/usr/lib/libz.a" "$PHI_SYSROOT/usr/include/zlib.h"
echo "== audit (must be clean)"
"$AUDIT" "$PHI_SYSROOT/usr/lib/libz.a"
