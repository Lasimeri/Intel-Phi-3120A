#!/usr/bin/env bash
# build.sh: generate, assemble and install libknc, the card's vector unit
# as a linkable library.
#
# The kernels are hand-encoded MVEX (host/crates/knc-mvex), so the whole
# library is one generated assembly file plus two headers: no C, no Rust,
# nothing for a compiler to get wrong. It installs into the host sysroot
# (for knc-cc) and into a package tree for /opt/phi (for the card's own
# clang), so `cc prog.c -lknc` works in both places. See build.md.
#
#   PHI_KNC_GEN   override the generator binary
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
OUT="$phi_build/userland/libknc"
PKG="$OUT/pkg"
AUDIT="$root/host/target/debug/phi-isa-audit"
GEN="${PHI_KNC_GEN:-$root/host/target/release/knc-mvex-gen}"

[ -x "$AUDIT" ] || { echo "build.sh: $AUDIT missing; run 'make build' first" >&2; exit 1; }
if [ -z "${PHI_KNC_GEN:-}" ]; then
    # Always rebuild. A stale generator would quietly emit the previous
    # encoder's bytes, and nothing downstream would notice.
    echo "== building the generator"
    (cd "$root/host" && cargo build --release -p knc-mvex)
fi
[ -x "$GEN" ] || { echo "build.sh: generator $GEN missing" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libc.a" ] || { echo "build.sh: no musl sysroot at $PHI_SYSROOT; run toolchain/musl/build.sh first" >&2; exit 1; }

rm -rf "$OUT"; mkdir -p "$OUT" "$PKG/opt/phi/usr/lib" "$PKG/opt/phi/usr/include"

echo "== generating kernels (32 widths, pack and unpack, plus the block copy)"
"$GEN" codec > "$OUT/knc_codec.S"
"$GEN" memcpy > "$OUT/knc_memcpy.S"
wc -l "$OUT/knc_codec.S" "$OUT/knc_memcpy.S" | sed 's/^/   /'

echo "== assembling"
for s in knc_codec knc_memcpy; do
    knc-cc -c -o "$OUT/$s.o" "$OUT/$s.S"
done
llvm-ar rcs "$OUT/libknc.a" "$OUT/knc_codec.o" "$OUT/knc_memcpy.o"
llvm-ranlib "$OUT/libknc.a"
ls -l "$OUT/libknc.a"

echo "== audit (must be clean: the encoder's bytes are not in the audit's tables)"
"$AUDIT" "$OUT/libknc.a"

echo "== installing into the sysroot $PHI_SYSROOT"
install -Dm644 "$OUT/libknc.a" "$PHI_SYSROOT/usr/lib/libknc.a"
install -Dm644 "$here/knc.h" "$PHI_SYSROOT/usr/include/knc.h"
install -Dm644 "$here/knc.hpp" "$PHI_SYSROOT/usr/include/knc.hpp"

echo "== packaging for /opt/phi on the card (its clang searches /opt/phi/usr)"
install -Dm644 "$OUT/libknc.a" "$PKG/opt/phi/usr/lib/libknc.a"
install -Dm644 "$here/knc.h" "$PKG/opt/phi/usr/include/knc.h"
install -Dm644 "$here/knc.hpp" "$PKG/opt/phi/usr/include/knc.hpp"
tar -czf "$OUT/phi-libknc.tar.gz" -C "$PKG" opt
ls -l "$OUT/phi-libknc.tar.gz"

cat <<'NOTE'

Installed. To use it:

  host, for the card   knc-cc -O2 prog.c -lknc
  card, natively       phi put .../phi-libknc.tar.gz /tmp/libknc.tar.gz
                       phi run sh -c 'tar -xzf /tmp/libknc.tar.gz -C /'
                       phi run sh -c 'cc -O2 prog.c -lknc'
NOTE
