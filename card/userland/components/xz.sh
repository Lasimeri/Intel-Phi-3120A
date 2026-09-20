#!/usr/bin/env bash
# xz.sh: build XZ Utils (liblzma and the xz driver) for the card, static,
# with threading. The version is pinned to match whatever the host runs so
# a card-versus-host measurement compares the same algorithm rather than
# two different ones. See xz.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
VER="${PHI_XZ_VERSION:-5.8.3}"
URL="https://github.com/tukaani-project/xz/releases/download/v$VER/xz-$VER.tar.gz"
SRC="$root/card/userland/build/xz-$VER"
OUT="$root/card/userland/build/xz"
AUDIT="$root/host/target/debug/phi-isa-audit"
[ -x "$AUDIT" ] || { echo "xz.sh: $AUDIT missing; run 'make build' first" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libc.a" ] || { echo "xz.sh: no musl sysroot at $PHI_SYSROOT; run toolchain/musl/build.sh first" >&2; exit 1; }

phi_fetch "xz-$VER.tar.gz" "$URL"
tarball="$phi_fetched"
rm -rf "$SRC"; mkdir -p "$SRC"
tar -xzf "$tarball" -C "$SRC" --strip-components=1
cd "$SRC"

# Cross-build notes:
# - The CRC32/CRC64 accelerations are CLMUL and SSE2; the card has neither,
#   so they are turned off explicitly rather than left to a runtime check
#   that would compile the instructions in regardless.
# - --disable-assembler keeps the hand-written x86 out of the build system,
#   but NOT out of the range decoder: src/liblzma/rangecoder/range_decoder.h
#   turns on inline x86-64 assembly whenever __x86_64__ is defined, using it
#   as a proxy for "has CMOV". The card is x86-64 without CMOV, so no
#   compiler flag can help and the audit finds 216 cmov instructions in
#   lzma_decoder.o. LZMA_RANGE_DECODER_CONFIG=0 selects the portable C path,
#   which is what every non-x86 target already uses.
# - Threading is the whole point of this build: xz -T N splits the input
#   into independent blocks, which is the only parallelism LZMA offers.
# - --disable-shared only governs liblzma; the xz driver still links
#   dynamically otherwise, and the card has no dynamic loader, so it fails
#   with a bare "not found" that looks like a missing file. -static has to
#   live in CC rather than LDFLAGS: libtool filters it out of the program
#   link (it reads -static as "prefer static libtool libraries"), and its
#   own -all-static is rejected by configure's plain compiler test.
# - configure cannot run its test programs when cross-compiling; the cache
#   variables below answer the two that matter for musl.
./configure \
    --host=x86_64-unknown-linux-musl \
    --build="$(./build-aux/config.guess)" \
    --prefix=/usr \
    --disable-shared --enable-static \
    --disable-nls --disable-doc --disable-assembler \
    --disable-clmul-crc \
    --enable-threads=posix \
    CC=knc-cc AR=llvm-ar RANLIB=llvm-ranlib STRIP=llvm-strip \
    CFLAGS="-O2" \
    CPPFLAGS="-DLZMA_RANGE_DECODER_CONFIG=0" \
    ac_cv_func_malloc_0_nonnull=yes \
    ac_cv_func_realloc_0_nonnull=yes \
    > configure.log 2>&1 || { tail -30 configure.log >&2; exit 1; }

# -all-static is libtool's flag for "link the program fully statically". It
# only works at make time: configure's own compiler test invokes clang
# directly, which rejects it, and plain -static in CC or LDFLAGS gets
# filtered out of the program link by libtool (it reads -static as "prefer
# static libtool libraries", not "static binary").
make -j"$(nproc)" LDFLAGS="-all-static" > make.log 2>&1 || { tail -40 make.log >&2; exit 1; }

# Belt and braces: verify the result is actually static, and relink by hand if
# libtool produced a dynamic binary anyway. The card has no dynamic loader, so
# a dynamic xz fails at exec with a bare "not found" that looks like a missing
# file rather than a link problem.
if head -c 20 src/xz/xz | grep -aq ELF && llvm-readelf -l src/xz/xz 2>/dev/null | grep -q INTERP; then
	echo "== libtool produced a dynamic binary; relinking statically by hand"
	objs=$(find src/xz -name '*.o' | sort | tr '\n' ' ')
	[ -n "$objs" ] || { echo "xz.sh: no objects in src/xz to relink" >&2; exit 1; }
	# shellcheck disable=SC2086
	knc-cc -static -o src/xz/xz $objs \
		src/liblzma/.libs/liblzma.a src/common/.libs/libcommon.a -lpthread
fi

mkdir -p "$OUT"
cp src/xz/xz "$OUT/xz"
llvm-strip "$OUT/xz" 2>/dev/null || true
# liblzma into the sysroot as well: anything else built for the card that
# wants compression can link it.
make install DESTDIR="$PHI_SYSROOT" > install.log 2>&1
ls -l "$OUT/xz" "$PHI_SYSROOT/usr/lib/liblzma.a"

echo "== audit (must be clean: 0 illegal)"
"$AUDIT" "$OUT/xz"
"$AUDIT" "$PHI_SYSROOT/usr/lib/liblzma.a"

echo
echo "xz.sh: built $VER for the card at $OUT/xz"
echo "xz.sh: push it with:  phi put $OUT/xz /opt/phi/bin/xz"
