#!/usr/bin/env bash
# ncurses.sh: build ncurses (wide-character, static) for the card into the
# sysroot, with the common terminal descriptions compiled in as fallbacks
# so that no terminfo database is needed on the card. Needed by htop and by
# CPython's curses module (glances). See ncurses.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
VER="${PHI_NCURSES_VERSION:-6.5}"
URL="https://ftp.gnu.org/gnu/ncurses/ncurses-$VER.tar.gz"
DL="$root/toolchain/build/downloads"
SRC="$root/card/userland/build/ncurses-$VER"
SUMS="$DL/SHA256SUMS"
AUDIT="$root/host/target/debug/phi-isa-audit"
mkdir -p "$DL"; touch "$SUMS"
tarball="$DL/ncurses-$VER.tar.gz"
if [ ! -s "$tarball" ]; then
    echo "== fetching $URL"
    curl -fL --retry 3 -o "$tarball.part" "$URL" && mv "$tarball.part" "$tarball"
fi
sum=$(sha256sum "$tarball" | awk '{print $1}')
if grep -q " ncurses-$VER.tar.gz\$" "$SUMS"; then
    want=$(grep " ncurses-$VER.tar.gz\$" "$SUMS" | awk '{print $1}')
    [ "$sum" = "$want" ] || { echo "SHA-256 mismatch for ncurses-$VER.tar.gz" >&2; exit 1; }
else
    echo "$sum  ncurses-$VER.tar.gz" >> "$SUMS"
fi
command -v tic > /dev/null || { echo "ncurses.sh: the host needs tic (pacman -S ncurses) to compile the fallback terminfo entries" >&2; exit 1; }
rm -rf "$SRC"; mkdir -p "$SRC"
tar -xzf "$tarball" -C "$SRC" --strip-components=1
cd "$SRC"
echo "== configuring"
# Cross build: the target library with knc-cc, the build-time tools with
# the host compiler; fallbacks are compiled from the host's terminfo with
# the host's tic. No shared library, no C++ binding, no programs (the
# card has busybox's clear/reset; tic is not needed without a database).
./configure --host=x86_64-linux-musl --build=x86_64-pc-linux-gnu --prefix=/usr \
    CC=knc-cc AR=llvm-ar RANLIB=llvm-ranlib STRIP=llvm-strip \
    --with-build-cc=/usr/bin/cc --with-build-cflags=-O2 \
    --with-tic-path=/usr/bin/tic --with-infocmp-path=/usr/bin/infocmp \
    --without-shared --with-normal --enable-widec --without-debug \
    --without-ada --without-cxx --without-cxx-binding --without-manpages \
    --without-tests --without-progs --disable-db-install --disable-stripping \
    --with-fallbacks=xterm,xterm-256color,xterm-color,vt100,vt102,linux,screen,screen-256color,tmux,tmux-256color,dumb,ansi \
    --with-default-terminfo-dir=/opt/phi/share/terminfo > configure.log 2>&1
echo "== building"
make -j"$(nproc)" > make.log 2>&1
make install.libs install.includes DESTDIR="$PHI_SYSROOT" > install.log 2>&1
ls -l "$PHI_SYSROOT/usr/lib/libncursesw.a" "$PHI_SYSROOT/usr/include/curses.h"
echo "== audit (must be clean)"
"$AUDIT" "$PHI_SYSROOT/usr/lib/libncursesw.a"
