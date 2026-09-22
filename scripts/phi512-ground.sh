#!/usr/bin/env bash
# phi512-ground.sh: check one computation against three independent
# executions of it, one of them on the card.
#
#   scripts/phi512-ground.sh
#
# The same degree-30 polynomial is evaluated three ways:
#
#   1. the host's own FMA3 hardware, through fmaf(), which is what real
#      fused-multiply-add silicon produces
#   2. AVX-512 machine code, executed on the host by phi512, which has no
#      AVX-512 hardware at all
#   3. AVX-512 machine code translated to the card's MVEX instruction set
#      by avx512-xlate and executed on the Xeon Phi's vector units
#
# Three different instruction sets, three different processors, one
# answer. Agreement to the bit is the claim; disagreement anywhere says
# which of the three is wrong.
#
# Needs the card up (phi status). See phi512-ground.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
green=$'\033[32m'; red=$'\033[31m'; bold=$'\033[1m'; off=$'\033[0m'
[ -t 1 ] || { green=; red=; bold=; off=; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

echo "${bold}1. host FMA3 hardware${off} (the reference)"
tcc -o gen "$root/tools/gen-avx512-vectors.c" -lm
./gen >/dev/null
echo "   wrote poly_expected.bin from fmaf() on this host's FMA3 unit"

echo "${bold}2. AVX-512 machine code, on a host with no AVX-512${off}"
gcc -O2 -o demo "$root/tools/avx512-demo.c" "$root/card/examples/avx512_poly.avx512.s"
if "$here/phi512.sh" ./demo > host.out 2>&1; then
    echo "   $(tail -1 host.out)"
else
    echo "   ${red}FAILED${off}"; cat host.out; exit 1
fi

echo "${bold}3. the same AVX-512, translated and run on the card${off}"
"$root/host/target/debug/avx512-xlate" "$root/card/examples/avx512_poly.avx512.s" \
    --name poly_kernel_x8 --out card_kernel.S 2>/dev/null
# The translated assembly must be byte-identical to what is committed,
# or this is testing something other than what ships.
if ! diff -q card_kernel.S "$root/card/examples/avx512_poly.S" >/dev/null; then
    echo "   ${red}the translator's output differs from card/examples/avx512_poly.S${off}"
    exit 1
fi
echo "   translated output matches the committed card/examples/avx512_poly.S"

ssh -o ConnectTimeout=10 phi 'mkdir -p /tmp/ground' >/dev/null
scp -O -q "$root/card/examples/avx512_poly.c" card_kernel.S x.bin coef.bin poly_expected.bin phi:/tmp/ground/
if ssh phi 'cd /tmp/ground && cc -O2 -o poly avx512_poly.c card_kernel.S -lpthread && ./poly 65536 1' > card.out 2>&1; then
    echo "   $(head -1 card.out)"
else
    echo "   ${red}FAILED on the card${off}"; cat card.out; exit 1
fi

echo
if grep -q "bit-identical" host.out && grep -q "bit-identical" card.out; then
    echo "${green}${bold}All three agree to the bit.${off}"
    echo "  host FMA3 hardware, host software emulation, and the card's vector units"
    echo "  produce identical results for the same AVX-512 program."
else
    echo "${red}${bold}They do not all agree.${off}"
    exit 1
fi
