#!/usr/bin/env bash
# fastlanes.sh: build FastLanes (github.com/cwida/FastLanes) for the card,
# with its 32-bit unffor hot path routed through the card's vector unit.
#
# Seven changes to the upstream tree, all applied here rather than kept as a
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

echo "== patch 1/7: <climits> in the CUDA common header"
# CHAR_BIT is used in a constant expression there and reaches it only
# through a transitive include on the compilers upstream tests with. Under
# this clang and libc++ it is undeclared, and the error cascades into a
# variable-length array diagnostic that -Werror makes fatal.
if ! grep -q '^#include <climits>' src/include/fls/cuda/common.hpp; then
    perl -pi -e 's{^#include "fls/cuda/config\.hpp"$}{#include "fls/cuda/config.hpp"\n#include <climits>}' \
        src/include/fls/cuda/common.hpp
fi
grep -n '#include <climits>' src/include/fls/cuda/common.hpp | sed 's/^/   /'

echo "== patch 2/7: value-initialise last_seen_val"
# The member initialiser was last_seen_val(0). The template is instantiated
# for std::string as well, where string(0) is string(nullptr) and undefined;
# clang says so through -Wnonnull, which -Werror makes fatal. Value
# initialisation is correct for every instantiation.
if grep -q 'last_seen_val(0)' src/table/stats.cpp; then
    perl -pi -e 's{last_seen_val\(0\)\s*// NOLINT}{last_seen_val {} /* value-initialised: also instantiated for std::string, where string(0) is string(nullptr) */}' \
        src/table/stats.cpp
fi
grep -n 'last_seen_val {}' src/table/stats.cpp | sed 's/^/   /'

echo "== patch 3/7: rename the generated uint32_t unffor dispatcher"
# One line. The generated switch over 33 widths stays exactly as it is and
# stays callable, which is what the equivalence check compares against.
if grep -q '^void unffor(const uint32_t\* FLS_RESTRICT a_in_p,$' src/alp/src/fastlanes_gen_unffor.cpp; then
    perl -pi -e 's{^void unffor\(const uint32_t\* FLS_RESTRICT a_in_p,$}{void unffor_scalar(const uint32_t* FLS_RESTRICT a_in_p,}' \
        src/alp/src/fastlanes_gen_unffor.cpp
fi
grep -c '^void unffor_scalar(const uint32_t\* FLS_RESTRICT a_in_p,$' src/alp/src/fastlanes_gen_unffor.cpp | sed 's/^/   renamed: /'

echo "== patch 4/7: append the MVEX dispatcher and link libknc"
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

echo "== patch 5/7: prefix-sum the candidate scan in enc_analyze_opr"
# enc_analyze_opr picks a frame of reference by scoring every pair (i, j)
# of distinct values in a vector, and find_best_option summed rep_vec over
# [i, j] on each call. That is cubic in the number of distinct values: a
# high cardinality vector has 1024 of them, so the inner loop ran about
# 1.8e8 times per vector per candidate encoding. Carrying prefix sums on
# the histogram turns the range sum into a subtraction. The same sum, so
# the encoder picks the same option and writes the same bytes; this is
# verified by comparing output files, not by argument.
AO_H=src/include/fls/expression/analyze_operator.hpp
AO_C=src/expression/analyze_operator.cpp
AO_I=src/expression/analyze_operator_impl.hpp

cat > "$OUT/patch5-prefix.inc" <<'PFXEOF'

	/* Prefix sums, so that summing rep_vec over a range is two loads
	 * rather than a loop. find_best_option is called once for every
	 * (i, j) pair of distinct values and summed the range itself, which
	 * made the analysis cubic in the number of distinct values in a
	 * vector. A high cardinality vector has 1024 of them, so that inner
	 * loop ran about 1.8e8 times per vector per candidate encoding. */
	prefix_vec.reserve(rep_vec.size() + 1);
	prefix_vec.push_back(0);
	uint32_t running {0};
	for (n_t i = 0; i < rep_vec.size(); ++i) {
		running += rep_vec[i];
		prefix_vec.push_back(running);
	}
PFXEOF

cat > "$OUT/patch5-range.inc" <<'RNGEOF'
	/* The same sum the loop here used to compute: rep_vec over
	 * [first_base_idx, next_base_idx]. See AnalyzeHistogram::Cal. */
	const auto&     prefix = histogram.prefix_vec;
	const vec_idx_t n_non_exceptions =
	    static_cast<vec_idx_t>(prefix[static_cast<n_t>(next_base_idx) + 1] - prefix[first_base_idx]);
RNGEOF

if ! grep -q prefix_vec "$AO_H"; then
    awk '
      { print }
      $0 == "\tstd::vector<uint16_t> rep_vec; //" {
          print "\tstd::vector<uint32_t> prefix_vec; // prefix_vec[k] = sum of rep_vec over [0, k); see Cal"
      }
    ' "$AO_H" > "$AO_H.new" && mv "$AO_H.new" "$AO_H"
fi
if ! grep -q prefix_vec "$AO_C"; then
    awk -v blk="$OUT/patch5-prefix.inc" '
      $0 == "\t\t\trep_vec.push_back(1);" { armed = 1 }
      armed == 1 && $0 == "}" {
          while ((getline l < blk) > 0) { print l }
          close(blk)
          armed = 2
          print
          next
      }
      { print }
      $0 == "\trep_vec.clear();" { print "\tprefix_vec.clear();" }
    ' "$AO_C" > "$AO_C.new" && mv "$AO_C.new" "$AO_C"
fi
if ! grep -q prefix_vec "$AO_I"; then
    awk -v blk="$OUT/patch5-range.inc" '
      $0 == "\tauto&   rep_vec     = histogram.rep_vec;" { next }
      skip > 0 { skip--; next }
      $0 == "\tvec_idx_t n_non_exceptions {0};" {
          while ((getline l < blk) > 0) { print l }
          close(blk)
          skip = 3   # the for, its body, its closing brace
          next
      }
      { print }
    ' "$AO_I" > "$AO_I.new" && mv "$AO_I.new" "$AO_I"
fi
grep -c prefix_vec "$AO_H" "$AO_C" "$AO_I" | sed 's/^/   /'

echo "== patch 6/7: bound the candidate scan by the exception limit"
# The scan keeps an option only when it leaves fewer than
# LOCAL_EXC_LIMIT_C exceptions. Two bounds follow from that guard alone:
# n_exceptions(i, j) is never below prefix[i], so the outer loop can stop
# as soon as prefix[i] reaches the limit; and n_exceptions falls as j
# grows, so the inner loop can start at the first j that clears the limit.
# Every pair skipped is one the guard would have rejected, so again the
# encoder writes the same bytes. On a vector of 1024 distinct values this
# is about 400 pairs instead of 524288.
cat > "$OUT/patch6-admissible.inc" <<'ADMEOF'
/* The smallest j for which the range [i, j] leaves fewer than
 * LOCAL_EXC_LIMIT_C values outside it. n_exceptions is
 * VEC_SZ - (prefix[j + 1] - prefix[i]) and falls as j grows, so every
 * smaller j fails the guard in Analyze and cannot change the answer,
 * and every larger one is still worth testing. */
inline vec_idx_t first_admissible_j(const std::vector<uint32_t>& prefix, n_t n_option, vec_idx_t i) {
	const uint32_t need = static_cast<uint32_t>(CFG::VEC_SZ) - (LOCAL_EXC_LIMIT_C - 1) + prefix[i];
	const auto     first = prefix.begin() + static_cast<std::ptrdiff_t>(i) + 1;
	const auto     last  = prefix.begin() + static_cast<std::ptrdiff_t>(n_option) + 1;
	const auto     it    = std::lower_bound(first, last, need);
	if (it == last) {
		return static_cast<vec_idx_t>(n_option);
	}
	return static_cast<vec_idx_t>((it - prefix.begin()) - 1);
}

ADMEOF

if ! grep -q first_admissible_j "$AO_I"; then
    awk -v blk="$OUT/patch6-admissible.inc" '
      $0 == "template <typename PT, bool IS_PATCHED>" && placed == 0 {
          while ((getline l < blk) > 0) { print l }
          close(blk)
          placed = 1
      }
      {
          line = $0
          sub(/^\t+/, "", line)
          indent = substr($0, 1, length($0) - length(line))
          if (line == "for (vec_idx_t i {0}; i < n_option; ++i) {") {
              print
              print indent "\tif (histogram.prefix_vec[i] >= LOCAL_EXC_LIMIT_C) {"
              print indent "\t\tbreak; /* n_exceptions is at least prefix_vec[i], whatever j is */"
              print indent "\t}"
              print indent "\tconst vec_idx_t j0 = first_admissible_j(histogram.prefix_vec, n_option, i);"
              next
          }
          if (line == "for (vec_idx_t j {i}; j < n_option; ++j) {") {
              print indent "for (vec_idx_t j {j0}; j < n_option; ++j) {"
              next
          }
          print
      }
    ' "$AO_I" > "$AO_I.new" && mv "$AO_I.new" "$AO_I"
fi
grep -c 'j {j0}' "$AO_I" | sed 's/^/   bounded scans: /'

echo "== patch 7/7: clear only the counter rows FSST12 dirtied"
# sizeof(Counters12) is 24 MB, and buildSymbol12Map memsets all of it once
# per round, four rounds per symbol table. On a real file that memset was
# 47.6 percent of the whole compression run on the card, and it cannot be
# made faster: a 24 MB clear runs at 4.82 GB/s there, which a plain scalar
# store loop already reaches, so it is the memory system and not the
# instruction stream (measured 2026-09-21, card/lib/knc-fls/fls_compress).
# The fix is to clear less. count2Inc(pos1, pos2) is only ever reached
# after count1Inc(pos1) in the same iteration, and count1High[pos1] is
# non-zero exactly when that symbol occurred, so the rows of count2 that
# can be dirty are exactly those with count1High[pos1] != 0. Clearing
# those, plus the two count1 arrays, leaves the structure as a full
# memset would, as long as it started zeroed: so the first round still
# does the full memset and every later round clears what it dirtied.
FS_H=src/include/fls/cor/prm/fsst12/libfsst12.hpp
FS_C=src/cor/prm/fsst12/libfsst12.cpp

cat > "$OUT/patch7-clear.inc" <<'CLREOF'
	/* Undo exactly what a round of compressCount can have written, given
	 * that this structure was zero before it. count2Inc(pos1, pos2) is
	 * only reached after count1Inc(pos1) in the same iteration, and
	 * count1High[pos1] is non-zero exactly when the symbol occurred (it
	 * is incremented early, see count1Inc), so a zero count1High[pos1]
	 * proves row pos1 of count2 is untouched. The whole structure is
	 * 24 MB and a sample dirties a few rows of it. */
	void clearTouched() {
		for (u32 pos1 = 0; pos1 < FSST12_CODE_MAX; pos1++) {
			if (count1High[pos1] == 0) {
				continue;
			}
			memset(count2High[pos1], 0, FSST12_CODE_MAX / 2);
			memset(count2Low[pos1], 0, FSST12_CODE_MAX);
		}
		memset(count1High, 0, FSST12_CODE_MAX);
		memset(count1Low, 0, FSST12_CODE_MAX);
	}

CLREOF

if ! grep -q clearTouched "$FS_H"; then
    awk -v blk="$OUT/patch7-clear.inc" '
      $0 == "\tvoid backup1(u8* buf) {" && placed == 0 {
          while ((getline l < blk) > 0) { print l }
          close(blk)
          placed = 1
      }
      { print }
    ' "$FS_H" > "$FS_H.new" && mv "$FS_H.new" "$FS_H"
fi
grep -c clearTouched "$FS_H" | sed 's/^/   header: /'

if ! grep -q clearTouched "$FS_C"; then
    awk '
      $0 == "#ifdef NONOPT_FSST12" && placed == 0 {
          print "\t/* The first round has to zero all 24 MB; see Counters12::clearTouched. */"
          print "\tbool counters_zeroed = false;"
          placed = 1
      }
      $0 == "\t\tmemset(&counters, 0, sizeof(Counters12));" {
          print "\t\tif (counters_zeroed) {"
          print "\t\t\tcounters.clearTouched();"
          print "\t\t} else {"
          print "\t\t\tmemset(&counters, 0, sizeof(Counters12));"
          print "\t\t\tcounters_zeroed = true;"
          print "\t\t}"
          next
      }
      { print }
    ' "$FS_C" > "$FS_C.new" && mv "$FS_C.new" "$FS_C"
fi
grep -c clearTouched "$FS_C" | sed 's/^/   source: /'

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

echo "== building the check, the benchmarks and the round trip"
mkdir -p "$PKG/opt/phi/bin"
for prog in fls_check fls_bench fls_roundtrip fls_compress; do
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
