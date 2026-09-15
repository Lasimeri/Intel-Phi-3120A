#!/usr/bin/env bash
# zlib.sh: build zlib for the card (static) into the sysroot. Needed by
# CPython's zlib module and by anything else that compresses. See zlib.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
VER="${PHI_ZLIB_VERSION:-1.3.1}"
URL="https://github.com/madler/zlib/releases/download/v$VER/zlib-$VER.tar.gz"
DL="$root/toolchain/build/downloads"
SRC="$root/card/userland/build/zlib-$VER"
SUMS="$DL/SHA256SUMS"
AUDIT="$root/host/target/debug/phi-isa-audit"
mkdir -p "$DL"; touch "$SUMS"
tarball="$DL/zlib-$VER.tar.gz"
if [ ! -s "$tarball" ]; then
    echo "== fetching $URL"
    curl -fL --retry 3 -o "$tarball.part" "$URL" && mv "$tarball.part" "$tarball"
fi
sum=$(sha256sum "$tarball" | awk '{print $1}')
if grep -q " zlib-$VER.tar.gz\$" "$SUMS"; then
    want=$(grep " zlib-$VER.tar.gz\$" "$SUMS" | awk '{print $1}')
    [ "$sum" = "$want" ] || { echo "SHA-256 mismatch for zlib-$VER.tar.gz" >&2; exit 1; }
else
    echo "$sum  zlib-$VER.tar.gz" >> "$SUMS"
fi
rm -rf "$SRC"; mkdir -p "$SRC"
tar -xzf "$tarball" -C "$SRC" --strip-components=1
cd "$SRC"
CC=knc-cc AR=llvm-ar RANLIB=llvm-ranlib ./configure --static --prefix=/usr > configure.log 2>&1
make -j"$(nproc)" > make.log 2>&1
make install DESTDIR="$PHI_SYSROOT" > install.log 2>&1
ls -l "$PHI_SYSROOT/usr/lib/libz.a" "$PHI_SYSROOT/usr/include/zlib.h"
echo "== audit (must be clean)"
"$AUDIT" "$PHI_SYSROOT/usr/lib/libz.a"
