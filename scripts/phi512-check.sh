#!/usr/bin/env bash
# phi512-check.sh: does the emulator agree with the hardware?
#
# Compiles the same C twice, once so it runs natively on this host and
# once so it needs AVX-512, then compares the output. The native build is
# the reference: it is the same source, the same compiler and the same
# optimisation level, so any difference is the emulator's.
#
#   scripts/phi512-check.sh            # the built-in conformance program
#   scripts/phi512-check.sh my.c       # any C file
#
# See phi512-check.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
src="${1:-$root/tools/avx512-conformance.c}"
[ -f "$src" ] || { echo "$0: no such file: $src" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
base=$(basename "$src" .c)

# -mno-avx512vl and friends keep the compiler to AVX-512F, which is the
# subset the emulator implements. Without them it reaches for BW and DQ
# instructions that are genuinely not covered.
cc_native=(gcc -O3 -march=native)
cc_512=(gcc -O3 -mavx512f -mno-avx512vl -mno-avx512bw -mno-avx512dq -mno-avx512cd)

"${cc_native[@]}" -o "$tmp/native" "$src" -lm
"${cc_512[@]}" -o "$tmp/avx512" "$src" -lm

# Whether this host has AVX-512 is read from the flags rather than found
# out by running the binary: executing it to see if it dies produces a
# core dump and a shell message, which is noise in a test harness.
if grep -qw avx512f /proc/cpuinfo; then
    echo "note: this host runs AVX-512 natively, so this compares the emulator against itself"
fi

"$tmp/native" > "$tmp/want" 2>&1 || { echo "$0: the native build failed to run" >&2; exit 1; }
if ! "$here/phi512.sh" "$tmp/avx512" > "$tmp/got" 2>"$tmp/err"; then
    echo "FAIL: the AVX-512 build did not complete under phi512"
    sed 's/^/  /' "$tmp/err" | head -5
    exit 1
fi

if diff -q "$tmp/want" "$tmp/got" >/dev/null; then
    n=$(wc -l < "$tmp/want")
    echo "PASS: $base, $n result line(s) identical to the native build"
else
    echo "FAIL: $base differs from the native build"
    diff "$tmp/want" "$tmp/got" | head -20
    exit 1
fi
