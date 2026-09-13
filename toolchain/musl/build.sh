#!/usr/bin/env bash
# build.sh: build musl with knc-cc into the card sysroot (toolchain/build/sysroot).
# Pinned version, trust-on-first-use checksum, SSE-dependent arch files
# removed before configure. See build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
VER="${PHI_MUSL_VERSION:-1.2.5}"
URL="https://musl.libc.org/releases/musl-$VER.tar.gz"
DL="$root/toolchain/build/downloads"
SRC="$root/toolchain/build/musl-$VER"
SYSROOT="${PHI_SYSROOT:-$root/toolchain/build/sysroot}"
CC="$root/toolchain/clang/knc-cc"
SUMS="$root/toolchain/build/downloads/SHA256SUMS"
mkdir -p "$DL"; touch "$SUMS"

tarball="$DL/musl-$VER.tar.gz"
if [ ! -s "$tarball" ]; then
    echo "== fetching $URL"
    curl -fL --retry 3 -o "$tarball.part" "$URL" && mv "$tarball.part" "$tarball"
fi
sum=$(sha256sum "$tarball" | awk '{print $1}')
if grep -q " musl-$VER.tar.gz\$" "$SUMS"; then
    want=$(grep " musl-$VER.tar.gz\$" "$SUMS" | awk '{print $1}')
    [ "$sum" = "$want" ] || { echo "SHA-256 mismatch for musl-$VER.tar.gz" >&2; exit 1; }
else
    echo "$sum  musl-$VER.tar.gz" >> "$SUMS"
fi

rm -rf "$SRC"; mkdir -p "$SRC"
tar -xzf "$tarball" -C "$SRC" --strip-components=1

# x86_64-specific sources that use SSE registers or instructions. Removing
# them makes musl's build pick the portable C implementations instead.
# Everything else under arch/x86_64 and src/*/x86_64 is integer, x87, or
# rep-string code, all of which Knights Corner executes.
for f in sqrt.c sqrtf.c lrint.c lrintf.c llrint.c llrintf.c; do
    rm -f "$SRC/src/math/x86_64/$f"
done

cd "$SRC"
echo "== configuring musl $VER with knc-cc"
CC="$CC" CFLAGS="-O2" ./configure \
    --target=x86_64-unknown-linux-musl \
    --prefix=/usr --syslibdir=/lib \
    --disable-shared --enable-static --disable-gcc-wrapper
echo "== building"
make -j"$(nproc)" >/dev/null
echo "== installing into $SYSROOT"
make DESTDIR="$SYSROOT" install >/dev/null

# Kernel UAPI headers: Arch's kernel-headers-musl package installs them
# under /usr/lib/musl/include; they are architecture-independent for
# x86-64 and are what musl expects to find alongside its own headers.
if [ -d /usr/lib/musl/include/linux ]; then
    cp -rn /usr/lib/musl/include/. "$SYSROOT/usr/include/"
else
    echo "WARNING: /usr/lib/musl/include missing (pacman -S extra/kernel-headers-musl); no <linux/*.h> in the sysroot" >&2
fi

echo "== audit"
"$root/host/target/debug/phi-isa-audit" --allow-suspect "$SYSROOT/usr/lib/libc.a" || {
    echo "musl contains KNC-illegal instructions; see the audit above" >&2; exit 1; }
echo "musl $VER installed in $SYSROOT"
