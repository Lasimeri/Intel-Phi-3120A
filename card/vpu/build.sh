#!/usr/bin/env bash
# Build the card-side offload worker, on the card, against the translated
# AVX-512 kernel. Run this on the card; scripts/phi-vpu.sh deploy copies
# the sources over and runs it.
#
# The kernel is card/examples/avx512_poly.S. When this directory has been
# copied to the card on its own, the deploy script puts a copy of that
# file next to this script, and that copy is used.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
kernel="$here/avx512_poly.S"
[ -f "$kernel" ] || kernel="$here/../examples/avx512_poly.S"
[ -f "$kernel" ] || { echo "build.sh: no avx512_poly.S next to this script or in ../examples" >&2; exit 1; }
cc -O2 -I"$here" -o "$here/phi-vpu-worker" "$here/vpu_worker.c" "$here/vpu_exec.c" "$kernel" -lpthread
echo "built $here/phi-vpu-worker"
