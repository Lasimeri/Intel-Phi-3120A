#!/usr/bin/env bash
# cpython.sh: cross-build CPython for the card as one static interpreter
# with every module the sysroot supports built in (no dlopen in static
# musl), install it under /opt/phi in a package tree, audit, and prove it by
# running the card binary on the host. See cpython.md.
#   PHI_PYTHON_VERSION   default 3.14.7 (must equal the host's python3)
#   PHI_PYTHON_SETUP     optional Modules/Setup fragment appended to
#                        Setup.local (extra built-in modules, e.g. psutil)
#   PHI_PYTHON_EXTRA_SRC optional directory copied into Modules/ first
# Output: card/userland/build/cpython/phi-python.tar.gz
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
VER="${PHI_PYTHON_VERSION:-3.14.7}"
URL="https://www.python.org/ftp/python/$VER/Python-$VER.tar.xz"
SRC="$root/card/userland/build/Python-$VER"
OUT="$root/card/userland/build/cpython"
AUDIT="$root/host/target/debug/phi-isa-audit"
BUILD_PY="${PHI_BUILD_PYTHON:-/usr/bin/python3}"
[ -x "$AUDIT" ] || { echo "cpython.sh: $AUDIT missing; run 'make build' first" >&2; exit 1; }
[ -x "$BUILD_PY" ] || { echo "cpython.sh: build interpreter $BUILD_PY missing (pacman -S python, or set PHI_BUILD_PYTHON)" >&2; exit 1; }
for lib in libc.a libz.a libncursesw.a; do
    [ -f "$PHI_SYSROOT/usr/lib/$lib" ] || { echo "cpython.sh: $lib missing from the sysroot $PHI_SYSROOT; run toolchain/musl/build.sh, zlib.sh and ncurses.sh first" >&2; exit 1; }
done
hostver=$("$BUILD_PY" -c 'import sys; print("%d.%d.%d" % sys.version_info[:3])')
[ "$hostver" = "$VER" ] || { echo "cpython.sh: the build interpreter is $hostver but the target is $VER; they must match (set PHI_PYTHON_VERSION or PHI_BUILD_PYTHON)" >&2; exit 1; }
mkdir -p "$OUT"
# Pinned download (toolchain/fetch.sh, toolchain/SHA256SUMS).
phi_fetch "Python-$VER.tar.xz" "$URL"
tarball="$phi_fetched"
# PHI_PYTHON_INCREMENTAL=1 keeps an existing configured tree and only
# refreshes the extra modules and reruns make (iterating on a Setup fragment).
if [ -z "${PHI_PYTHON_INCREMENTAL:-}" ] || [ ! -f "$SRC/Makefile" ]; then
    rm -rf "$SRC"; mkdir -p "$SRC"
    tar -xJf "$tarball" -C "$SRC" --strip-components=1
    fresh=1
else
    fresh=0
fi
cd "$SRC"
# Every module configure finds is built into the interpreter instead of as
# a shared object: static musl has no dlopen.
# Module build type: MODULE_BUILDTYPE=static is passed to configure below.
[ -f Modules/Setup.stdlib ] && sed -i 's/^\*shared\*$/*static*/' Modules/Setup.stdlib
# _hmac (new in 3.14) compiles the same HACL* hash sources as _md5, _sha1,
# _sha2, _sha3 and _blake2; fine as separate shared objects, duplicate
# symbols in one static binary. Python's hmac module works without it.
sed -i 's/^\(@MODULE__HMAC_TRUE@\)_hmac /\1#_hmac /' Modules/Setup.stdlib.in
[ -f Modules/Setup.stdlib ] && sed -i 's/^_hmac /#_hmac /' Modules/Setup.stdlib
if [ -n "${PHI_PYTHON_EXTRA_SRC:-}" ]; then
    rm -rf "Modules/$(basename "$PHI_PYTHON_EXTRA_SRC")"
    cp -a "$PHI_PYTHON_EXTRA_SRC" "Modules/$(basename "$PHI_PYTHON_EXTRA_SRC")"
fi
# Setup.local is written whole: the stock file is a comment header only.
if [ -n "${PHI_PYTHON_SETUP:-}" ]; then
    { echo "*static*"; cat "$PHI_PYTHON_SETUP"; } > Modules/Setup.local
else
    : > Modules/Setup.local
fi
echo "== configuring"
# Cross build: answers configure cannot get by running card code (it could,
# the card binaries run here, but configure does not know that).
if [ "$fresh" = 1 ]; then
# Only the sysroot may answer pkg-config queries: the host's lzma, bzip2,
# zstd, openssl or sqlite would otherwise be believed present.
mkdir -p "$PHI_SYSROOT/usr/lib/pkgconfig"
export PKG_CONFIG_LIBDIR="$PHI_SYSROOT/usr/lib/pkgconfig" PKG_CONFIG_PATH=""
./configure --host=x86_64-unknown-linux-musl --build=x86_64-pc-linux-gnu \
    --prefix=/opt/phi --with-build-python="$BUILD_PY" \
    MODULE_BUILDTYPE=static \
    CC=knc-cc AR=llvm-ar RANLIB=llvm-ranlib READELF=llvm-readelf \
    CFLAGS="-O2" LDFLAGS="-static" LINKFORSHARED=" " \
    --disable-shared --disable-ipv6 --disable-test-modules --without-ensurepip \
    --without-mimalloc \
    --without-static-libpython \
    ac_cv_file__dev_ptmx=yes ac_cv_file__dev_ptc=no \
    ac_cv_buggy_getaddrinfo=no ac_cv_working_tzset=yes \
    ac_cv_little_endian_double=yes ac_cv_big_endian_double=no ac_cv_mixed_endian_double=no \
    ax_cv_check_cflags__Werror__msse__msse2__msse3__msse4_1__msse4_2=no ax_cv_check_cflags__Werror__mavx2=no \
    > "$OUT/configure.log" 2>&1 || { tail -20 "$OUT/configure.log"; exit 1; }
fi
echo "== building"
make -j"$(nproc)" > "$OUT/make.log" 2>&1 || { grep -n "error" "$OUT/make.log" | head -20; exit 1; }
echo "== audit (must be clean)"
"$AUDIT" python
echo "== the card interpreter runs on the host"
./python -E -c 'import sys, zlib, math, curses, json, socket, threading; print(sys.version.split()[0], "modules ok, pi =", math.pi)'
echo "== installing into the package tree"
rm -rf "$OUT/root"
make install DESTDIR="$OUT/root" > "$OUT/install.log" 2>&1 || { tail -10 "$OUT/install.log"; exit 1; }
llvm-strip "$OUT/root/opt/phi/bin/python$(echo "$VER" | cut -d. -f1-2)"
rm -rf "$OUT/root/opt/phi/lib/python3."*/test "$OUT/root/opt/phi/lib/python3."*/idlelib "$OUT/root/opt/phi/lib/python3."*/tkinter "$OUT/root/opt/phi/lib/python3."*/turtledemo
(cd "$OUT/root" && tar -czf "$OUT/phi-python.tar.gz" opt)
ls -l "$OUT/phi-python.tar.gz"; du -sh "$OUT/root/opt/phi" | cut -f1
