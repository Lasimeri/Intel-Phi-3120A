#!/usr/bin/env bash
# clang-push.sh: load the packaged native clang onto the running card
# through phictl (no SSH): the tarball goes to /tmp and unpacks into
# /opt/phi, in the card's RAM. Needs `phictl boot --serve` running.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
P="$phi_root/host/target/debug/phictl"
PKG="$phi_root/card/userland/build/clang/phi-clang.tar.gz"
[ -s "$PKG" ] || { echo "clang-push.sh: $PKG missing; run clang.sh" >&2; exit 1; }
"$P" status
"$P" put "$PKG" /tmp/phi-clang.tar.gz
"$P" exec -- sh -c 'cd / && tar -xzf /tmp/phi-clang.tar.gz && rm /tmp/phi-clang.tar.gz && ls /opt/phi/bin | head -3'
"$P" exec -- sh -c 'printf "#include <stdio.h>\nint main(void){printf(\"compiled on the card\\\\n\");return 0;}\n" > /tmp/h.c && /opt/phi/bin/cc -o /tmp/h /tmp/h.c && /tmp/h'
