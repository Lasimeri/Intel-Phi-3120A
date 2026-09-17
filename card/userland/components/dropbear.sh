#!/usr/bin/env bash
# dropbear.sh: build a static dropbear multi-call binary (dropbear,
# dropbearkey, dbclient, scp) for the card with knc-cc, audit it, and
# generate the card's host keys with it (the card binary runs on the host:
# same instruction subset). Output: card/userland/build/dropbear/. See
# dropbear.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
VER="${PHI_DROPBEAR_VERSION:-2025.88}"
URL="https://matt.ucc.asn.au/dropbear/releases/dropbear-$VER.tar.bz2"
SRC="$root/card/userland/build/dropbear-$VER"
OUT="$root/card/userland/build/dropbear"
AUDIT="$root/host/target/debug/phi-isa-audit"
[ -x "$AUDIT" ] || { echo "dropbear.sh: $AUDIT missing; run 'make build' first" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libc.a" ] || { echo "dropbear.sh: no musl sysroot at $PHI_SYSROOT; run toolchain/musl/build.sh first" >&2; exit 1; }
mkdir -p "$OUT/keys"
# Pinned download (toolchain/fetch.sh, toolchain/SHA256SUMS).
phi_fetch "dropbear-$VER.tar.bz2" "$URL"
tarball="$phi_fetched"
rm -rf "$SRC"; mkdir -p "$SRC"
tar -xjf "$tarball" -C "$SRC" --strip-components=1
cd "$SRC"
echo "== configuring"
# Post-quantum key exchange off: the reference sntrup761 code contains
# x86-64 inline assembly with cmov (supercop crypto_int16 helpers), which
# the card lacks; the host negotiates curve25519 instead. See dropbear.md.
cat > localoptions.h <<'LOCAL'
#define DROPBEAR_SNTRUP761 0
#define DROPBEAR_MLKEM768 0
LOCAL
# No zlib (nothing else on the card needs it yet), no utmp/wtmp/lastlog
# (the initramfs has none of those files), no syslog (no syslogd). The
# card's compiler is given as CC; --host keeps configure from running
# host-specific probes that assume glibc.
./configure --host=x86_64-linux-musl --build=x86_64-pc-linux-gnu \
    CC=knc-cc AR=llvm-ar RANLIB=llvm-ranlib STRIP=llvm-strip \
    --disable-zlib --enable-static --disable-syslog \
    --disable-lastlog --disable-utmp --disable-utmpx --disable-wtmp --disable-wtmpx \
    --disable-loginfunc --disable-pututline --disable-pututxline > "$OUT/configure.log" 2>&1
echo "== building"
make -j"$(nproc)" PROGRAMS="dropbear dropbearkey dbclient scp" MULTI=1 STATIC=1 > "$OUT/make.log" 2>&1
cp dropbearmulti "$OUT/dropbearmulti"
echo "== audit (must be clean)"
"$AUDIT" "$OUT/dropbearmulti"
echo "== host keys"
for t in ed25519 rsa; do
    k="$OUT/keys/dropbear_${t}_host_key"
    [ -s "$k" ] || "$OUT/dropbearmulti" dropbearkey -t "$t" -f "$k" > /dev/null
done
ls -l "$OUT/dropbearmulti" "$OUT/keys"
echo "dropbear $VER built; the initramfs build picks it up from $OUT"
