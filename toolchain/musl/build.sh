#!/usr/bin/env bash
# build.sh: build musl with knc-cc into the card sysroot (toolchain/build/sysroot).
# Pinned version and SHA-256 (toolchain/SHA256SUMS), XMM-using arch files
# removed, project patches applied, archive audited. See build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../env.sh"
root="$phi_root"
here="$root/toolchain/musl"
VER="${PHI_MUSL_VERSION:-1.2.5}"
URL="https://musl.libc.org/releases/musl-$VER.tar.gz"
SRC="$root/toolchain/build/musl-$VER"
SYSROOT="$PHI_SYSROOT"
AUDIT="$root/host/target/debug/phi-isa-audit"
[ -x "$AUDIT" ] || { echo "build.sh: $AUDIT missing; run 'make build' first (the audit tool is a prerequisite of every card build)" >&2; exit 1; }

# Pinned download (toolchain/fetch.sh, toolchain/SHA256SUMS).
phi_fetch "musl-$VER.tar.gz" "$URL"
tarball="$phi_fetched"

rm -rf "$SRC"; mkdir -p "$SRC"
tar -xzf "$tarball" -C "$SRC" --strip-components=1

# 1. Drop every x86_64 math source that touches XMM registers. musl's build
#    uses an arch file when present and the portable C file otherwise, so
#    deleting is the whole port. The rest of arch/x86_64 and src/*/x86_64 is
#    x87, rep-string, or integer code, all executed by Knights Corner.
echo "== dropping XMM-using arch files"
for f in "$SRC"/src/math/x86_64/*; do
    if grep -qiE '"[=+]?x"|%xmm|cvtsd2si|cvtss2si|sqrtsd|sqrtss|andps|pcmpeqd|fcomi|fucomi|fcmov|cmov|pause|prefetch|mfence|lfence|sfence|clflush' "$f"; then
        echo "   $(basename "$f")"; rm -f "$f"
    fi
done

# 2. Project patches, in SERIES order (toolchain/musl/patches/README.md).
while IFS= read -r p; do
    case "$p" in ''|'#'*) continue ;; esac
    echo "== applying $p"
    patch -p1 -d "$SRC" < "$here/patches/$p" > /dev/null
done < "$here/patches/SERIES"

# 3. Configure, build, install. AR/RANLIB are named explicitly because musl
#    derives them from the --target prefix otherwise.
cd "$SRC"
echo "== configuring musl $VER with knc-cc"
CC=knc-cc AR=llvm-ar RANLIB=llvm-ranlib CFLAGS="-O2" ./configure \
    --target=x86_64-unknown-linux-musl \
    --prefix=/usr --syslibdir=/lib \
    --disable-shared --enable-static --disable-gcc-wrapper > configure.log
echo "== building"
make -j"$(nproc)" > build.log
echo "== installing into $SYSROOT"
make DESTDIR="$SYSROOT" install > install.log

# 4. Kernel UAPI headers from Arch's kernel-headers-musl package.
if [ -d /usr/lib/musl/include/linux ]; then
    cp -rn /usr/lib/musl/include/. "$SYSROOT/usr/include/"
else
    echo "WARNING: /usr/lib/musl/include missing (pacman -S extra/kernel-headers-musl); no <linux/*.h> in the sysroot" >&2
fi

# 5. Audit every member of libc.a; one illegal instruction fails the build.
echo "== audit"
"$AUDIT" "$SYSROOT/usr/lib/libc.a"
echo "musl $VER installed in $SYSROOT"
