#!/usr/bin/env bash
# busybox.sh: build a static busybox for the card with knc-cc, audit it,
# and run it on the host. Output: card/userland/build/busybox/busybox.
# See busybox.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
VER="${PHI_BUSYBOX_VERSION:-1.37.0}"
URL="https://busybox.net/downloads/busybox-$VER.tar.bz2"
SRC="$root/card/userland/build/busybox-$VER"
OUT="$root/card/userland/build/busybox"
AUDIT="$root/host/target/debug/phi-isa-audit"
[ -x "$AUDIT" ] || { echo "busybox.sh: $AUDIT missing; run 'make build' first" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libc.a" ] || { echo "busybox.sh: no musl sysroot at $PHI_SYSROOT; run toolchain/musl/build.sh first" >&2; exit 1; }
mkdir -p "$OUT"

# Pinned download (toolchain/fetch.sh, toolchain/SHA256SUMS).
phi_fetch "busybox-$VER.tar.bz2" "$URL"
tarball="$phi_fetched"

rm -rf "$SRC"; mkdir -p "$SRC"
tar -xjf "$tarball" -C "$SRC" --strip-components=1
cd "$SRC"

echo "== configuring"
make defconfig > /dev/null
# Static, no debug, and drop applets that need kernel features the card
# lacks or headers that fight newer UAPI (tc, the wireless and hardware
# clock tools, kernel module loaders are kept).
set_cfg() { sed -i "s/^# $1 is not set/$1=y/; s/^$1=.*/$1=$2/" .config; grep -q "^$1=" .config || echo "$1=$2" >> .config; }
unset_cfg() { sed -i "s/^$1=.*/# $1 is not set/" .config; }
set_cfg CONFIG_STATIC y
unset_cfg CONFIG_DEBUG
unset_cfg CONFIG_TC
unset_cfg CONFIG_HWCLOCK
unset_cfg CONFIG_RDATE
unset_cfg CONFIG_IWCONFIG
unset_cfg CONFIG_UDHCPC
unset_cfg CONFIG_UDHCPD
unset_cfg CONFIG_FEATURE_UDHCP_RFC3397
unset_cfg CONFIG_LOADFONT
unset_cfg CONFIG_SETFONT
unset_cfg CONFIG_FBSPLASH
unset_cfg CONFIG_FBSET
unset_cfg CONFIG_MOUNT_NFS
unset_cfg CONFIG_FEATURE_MOUNT_NFS
unset_cfg CONFIG_SELINUX
# Hand-written SHA-NI/SSE hash assembly with runtime dispatch; the audit
# cannot know it never runs, and KNC has neither extension.
unset_cfg CONFIG_SHA1_HWACCEL
unset_cfg CONFIG_SHA256_HWACCEL
make oldconfig < /dev/null > /dev/null

echo "== building"
make -j"$(nproc)" CC=knc-cc HOSTCC=cc AR=llvm-ar STRIP=llvm-strip \
    CONFIG_EXTRA_CFLAGS="-O2" busybox > build.log 2>&1 || { tail -30 build.log; exit 1; }
cp busybox "$OUT/busybox"
ls -l "$OUT/busybox"

echo "== audit"
"$AUDIT" "$OUT/busybox"
echo "== run on host"
"$OUT/busybox" echo "busybox $VER for the card runs on the host"
"$OUT/busybox" sh -c 'echo "shell ok: $((6*7))"; uname -m; ls -d /proc | head -1'
"$OUT/busybox" --list | wc -l | xargs echo "applets:"
