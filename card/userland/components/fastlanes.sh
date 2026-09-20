#!/usr/bin/env bash
# fastlanes.sh: build FastLanes (github.com/cwida/FastLanes) for the card,
# with its 32-bit unffor hot path routed through the card's vector unit.
#
# Four changes to the upstream tree, all applied here rather than kept as a
# fork, and all idempotent. See fastlanes.md for why each one is needed.
#
#   PHI_FASTLANES_COMMIT   default f0edc1020a538f1f8098640fce8347c9ac247a0d
# Output: card/userland/build/fastlanes/phi-fastlanes.tar.gz
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
SHA="${PHI_FASTLANES_COMMIT:-f0edc1020a538f1f8098640fce8347c9ac247a0d}"
URL="https://github.com/cwida/FastLanes/archive/$SHA.tar.gz"
SRC="$root/card/userland/build/fastlanes-$SHA"
OUT="$root/card/userland/build/fastlanes"
BLD="$OUT/build"
PKG="$OUT/pkg"
AUDIT="$root/host/target/debug/phi-isa-audit"
SHIM="$root/card/lib/knc-fls"

[ -x "$AUDIT" ] || { echo "fastlanes.sh: $AUDIT missing; run 'make build' first" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libc.a" ] || { echo "fastlanes.sh: no musl sysroot at $PHI_SYSROOT; run toolchain/musl/build.sh first" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libknc.a" ] || { echo "fastlanes.sh: libknc.a not in the sysroot; run card/lib/knc/build.sh first" >&2; exit 1; }
grep -q knc_fls_unffor "$PHI_SYSROOT/usr/include/knc.h" || { echo "fastlanes.sh: the installed knc.h has no knc_fls_unffor; rebuild libknc" >&2; exit 1; }
command -v cmake >/dev/null || { echo "fastlanes.sh: cmake not found" >&2; exit 1; }
command -v tcc >/dev/null || { echo "fastlanes.sh: tcc not found; it writes the example dataset" >&2; exit 1; }

mkdir -p "$OUT"
# Pinned download (toolchain/fetch.sh, toolchain/SHA256SUMS).
phi_fetch "fastlanes-$SHA.tar.gz" "$URL"
tarball="$phi_fetched"
rm -rf "$SRC"; mkdir -p "$SRC"
tar -xzf "$tarball" -C "$SRC" --strip-components=1
cd "$SRC"

echo "== patch 1/4: <climits> in the CUDA common header"
# CHAR_BIT is used in a constant expression there and reaches it only
# through a transitive include on the compilers upstream tests with. Under
# this clang and libc++ it is undeclared, and the error cascades into a
# variable-length array diagnostic that -Werror makes fatal.
if ! grep -q '^#include <climits>' src/include/fls/cuda/common.hpp; then
    perl -pi -e 's{^#include "fls/cuda/config\.hpp"$}{#include "fls/cuda/config.hpp"\n#include <climits>}' \
        src/include/fls/cuda/common.hpp
fi
grep -n '#include <climits>' src/include/fls/cuda/common.hpp | sed 's/^/   /'

echo "== patch 2/4: value-initialise last_seen_val"
# The member initialiser was last_seen_val(0). The template is instantiated
# for std::string as well, where string(0) is string(nullptr) and undefined;
# clang says so through -Wnonnull, which -Werror makes fatal. Value
# initialisation is correct for every instantiation.
if grep -q 'last_seen_val(0)' src/table/stats.cpp; then
    perl -pi -e 's{last_seen_val\(0\)\s*// NOLINT}{last_seen_val {} /* value-initialised: also instantiated for std::string, where string(0) is string(nullptr) */}' \
        src/table/stats.cpp
fi
grep -n 'last_seen_val {}' src/table/stats.cpp | sed 's/^/   /'

echo "== patch 3/4: rename the generated uint32_t unffor dispatcher"
# One line. The generated switch over 33 widths stays exactly as it is and
# stays callable, which is what the equivalence check compares against.
if grep -q '^void unffor(const uint32_t\* FLS_RESTRICT a_in_p,$' src/alp/src/fastlanes_gen_unffor.cpp; then
    perl -pi -e 's{^void unffor\(const uint32_t\* FLS_RESTRICT a_in_p,$}{void unffor_scalar(const uint32_t* FLS_RESTRICT a_in_p,}' \
        src/alp/src/fastlanes_gen_unffor.cpp
fi
grep -c '^void unffor_scalar(const uint32_t\* FLS_RESTRICT a_in_p,$' src/alp/src/fastlanes_gen_unffor.cpp | sed 's/^/   renamed: /'

echo "== patch 4/4: append the MVEX dispatcher and link libknc"
# Appending rather than adding a source file keeps CMake out of it: the
# generated translation unit is already in the build.
if ! grep -q knc_fls_unffor src/alp/src/fastlanes_gen_unffor.cpp; then
    printf '\n' >> src/alp/src/fastlanes_gen_unffor.cpp
    cat "$SHIM/fls_unffor.cpp" >> src/alp/src/fastlanes_gen_unffor.cpp
fi
if ! grep -q 'knc  # added by fastlanes.sh' src/CMakeLists.txt; then
    cat >> src/CMakeLists.txt <<'CMEOF'

# Added by card/userland/components/fastlanes.sh: the 32-bit unffor path is
# in libknc, so every consumer of FastLanes needs it on the link line.
target_link_libraries(FastLanes PUBLIC
        knc  # added by fastlanes.sh
)
CMEOF
fi
tail -5 src/CMakeLists.txt | sed 's/^/   /'

echo "== configuring for the card"
rm -rf "$BLD"; mkdir -p "$BLD"
cmake -S "$SRC" -B "$BLD" \
    -DCMAKE_TOOLCHAIN_FILE="$root/toolchain/cmake/knc.cmake" \
    -DCMAKE_BUILD_TYPE=Release \
    -DFLS_BUILD_SHARED_LIBS=OFF \
    -DFLS_ENABLE_INSTALL=OFF \
    > "$OUT/cmake.log" 2>&1 || { tail -30 "$OUT/cmake.log"; exit 1; }
echo "   configured; log in $OUT/cmake.log"

echo "== building libFastLanes.a"
cmake --build "$BLD" --target FastLanes -j "$(nproc)" > "$OUT/build.log" 2>&1 \
    || { tail -40 "$OUT/build.log"; exit 1; }
LIB=$(find "$BLD" -name libFastLanes.a | head -1)
ALP=$(find "$BLD" -name libfls_alp_primitive.a | head -1)
ls -l "$LIB" | sed 's/^/   /'

echo "== audit (no instruction the card cannot execute)"
"$AUDIT" "$LIB"

echo "== building the check, the benchmark and the round trip"
mkdir -p "$PKG/opt/phi/bin"
for prog in fls_check fls_bench fls_roundtrip; do
    knc-c++ -O2 -std=c++20 -static -I "$SRC/src/include" \
        -o "$PKG/opt/phi/bin/$prog" "$SHIM/$prog.cpp" "$LIB" "$ALP" -lknc
    "$AUDIT" "$PKG/opt/phi/bin/$prog" | tail -1 | sed "s|^|   $prog: |"
done

echo "== the example dataset (tcc, tools/fls-example-csv.c)"
DS="$PKG/opt/phi/share/fls-example"
mkdir -p "$DS"
tcc -run "$root/tools/fls-example-csv.c" "${PHI_FASTLANES_ROWS:-60000}" > "$DS/data.csv"
cat > "$DS/schema.json" <<'JSONEOF'
{
  "columns": [
    { "name": "a", "type": "INT", "nullability": "NOT NULL" },
    { "name": "b", "type": "INT", "nullability": "NOT NULL" },
    { "name": "c", "type": "INT", "nullability": "NOT NULL" }
  ]
}
JSONEOF
wc -l "$DS/data.csv" | sed "s/^/   /"

echo "== packaging"
mkdir -p "$PKG/opt/phi/usr/lib" "$PKG/opt/phi/usr/include"
cp "$LIB" "$ALP" "$PKG/opt/phi/usr/lib/"
cp -r "$SRC/src/include/." "$PKG/opt/phi/usr/include/"
tar -czf "$OUT/phi-fastlanes.tar.gz" -C "$PKG" opt
ls -l "$OUT/phi-fastlanes.tar.gz"

cat <<'NOTE'

Built. To check it on the card:

  phi put .../phi-fastlanes.tar.gz /tmp/fastlanes.tar.gz
  phi run sh -c 'tar -xzf /tmp/fastlanes.tar.gz -C /'
  phi run /opt/phi/bin/fls_check
  phi run /opt/phi/bin/fls_bench
  phi run /opt/phi/bin/fls_roundtrip /opt/phi/share/fls-example /tmp
NOTE
