#!/usr/bin/env bash
# zstd.sh: build Zstandard (libzstd and the zstd driver) for the card,
# static, with multithreading. Pinned to the host's version so a card
# against host measurement compares the same algorithm. See zstd.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
VER="${PHI_ZSTD_VERSION:-1.5.7}"
URL="https://github.com/facebook/zstd/releases/download/v$VER/zstd-$VER.tar.gz"
SRC="$root/card/userland/build/zstd-$VER"
OUT="$root/card/userland/build/zstd"
AUDIT="$root/host/target/debug/phi-isa-audit"
[ -x "$AUDIT" ] || { echo "zstd.sh: $AUDIT missing; run 'make build' first" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libc.a" ] || { echo "zstd.sh: no musl sysroot at $PHI_SYSROOT; run toolchain/musl/build.sh first" >&2; exit 1; }

phi_fetch "zstd-$VER.tar.gz" "$URL"
tarball="$phi_fetched"
rm -rf "$SRC"; mkdir -p "$SRC"
tar -xzf "$tarball" -C "$SRC" --strip-components=1
cd "$SRC"

# Build notes:
# - ZSTD_DISABLE_ASM keeps lib/decompress/huf_decompress_amd64.S out: it is
#   hand-written x86-64.
# - DYNAMIC_BMI2=0 is the one that matters. zstd compiles BMI2 variants of
#   its entropy decoders with __attribute__((target("lzcnt,bmi,bmi2"))) and
#   dispatches on CPUID at run time. A function-level target attribute
#   OVERRIDES the command line, so -mno-bmi2 does not reach those bodies,
#   and because the attribute resets the feature baseline it brings CMOV
#   back as well: the audit found 660 shlx, 27 lzcnt, 20 tzcnt and 20 cmov.
#   The CPUID guard means the card would never call them, but they are in
#   the binary, and this project audits linked executables with no
#   exceptions.
# - HAVE_ZLIB/LZMA/LZ4=0 drops the optional format wrappers; none of them
#   are built for the card and configure would otherwise find the host's.
# - -static because the card has no dynamic loader. zstd's Makefile passes
#   LDFLAGS through to the program link directly, with no libtool in the
#   way, so unlike xz this needs no relink fallback.
# - ZSTD_MULTITHREAD is what gives the driver -T; it is the whole point of
#   running this on a 228-thread machine.
make -j"$(nproc)" \
    CC=knc-cc AR=llvm-ar RANLIB=llvm-ranlib \
    CPPFLAGS="-DZSTD_DISABLE_ASM -DZSTD_MULTITHREAD -DDYNAMIC_BMI2=0" \
    CFLAGS="-O2" \
    LDFLAGS="-static -pthread" \
    HAVE_ZLIB=0 HAVE_LZMA=0 HAVE_LZ4=0 \
    zstd > make.log 2>&1 || { tail -40 make.log >&2; exit 1; }

mkdir -p "$OUT"
cp programs/zstd "$OUT/zstd"
llvm-strip "$OUT/zstd" 2>/dev/null || true
# The static library into the sysroot for anything else built for the card.
make -C lib libzstd.a \
    CC=knc-cc AR=llvm-ar RANLIB=llvm-ranlib \
    CPPFLAGS="-DZSTD_DISABLE_ASM -DZSTD_MULTITHREAD -DDYNAMIC_BMI2=0" CFLAGS="-O2" \
    > lib.log 2>&1 || true
[ -f lib/libzstd.a ] && cp lib/libzstd.a "$PHI_SYSROOT/usr/lib/libzstd.a"
[ -f lib/zstd.h ] && cp lib/zstd.h "$PHI_SYSROOT/usr/include/zstd.h"

file "$OUT/zstd"
echo "== audit (must be clean: 0 illegal)"
"$AUDIT" "$OUT/zstd"

echo
echo "zstd.sh: built $VER for the card at $OUT/zstd"
echo "zstd.sh: push it with:  phi put $OUT/zstd /opt/phi/bin/zstd"
