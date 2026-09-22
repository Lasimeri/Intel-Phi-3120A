#!/usr/bin/env bash
# phi512.sh: run a program that needs AVX-512 on a host that has none.
#
#   scripts/phi512.sh ./my-avx512-program [args...]
#
# The program is not modified, recompiled, or asked to cooperate. It
# executes an AVX-512 instruction, this host refuses it, and libphi512
# performs the instruction and lets the program continue.
#
#   --verbose   report how many AVX-512 instructions were performed
#   --check     say whether this host needs the library at all, and exit
#
# See phi512.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)

verbose=0
while [ $# -gt 0 ]; do
    case "$1" in
        --verbose|-v) verbose=1; shift ;;
        --check)
            if grep -qw avx512f /proc/cpuinfo; then
                echo "this host has AVX-512; the library is unnecessary here"
                exit 0
            fi
            echo "this host has no AVX-512 (no avx512f flag): programs that use it need this wrapper"
            exit 0
            ;;
        --) shift; break ;;
        -*) echo "$0: unknown option $1" >&2; exit 2 ;;
        *) break ;;
    esac
done
[ $# -ge 1 ] || { echo "usage: $0 [--verbose] PROGRAM [args...]" >&2; exit 2; }

# LD_PRELOAD splits its value on spaces and colons, and this repository is
# normally cloned to a path with a space in it ("Intel Phi 3120A"). The
# space-free symlink that toolchain/env.sh maintains is the way in; if it is
# missing, make one, because there is no quoting that would help here.
cache="${XDG_CACHE_HOME:-$HOME/.cache}/intel-phi-3120a"
if [ ! -e "$cache" ]; then ln -sfn "$root" "$cache"; fi

lib=""
for cand in "$cache/host/target/release/libphi512.so" "$cache/host/target/debug/libphi512.so"; do
    [ -f "$cand" ] && { lib="$cand"; break; }
done
[ -n "$lib" ] || { echo "$0: libphi512.so not built; run: make build" >&2; exit 1; }
case "$lib" in *" "*) echo "$0: the library path still contains a space, LD_PRELOAD cannot express it: $lib" >&2; exit 1 ;; esac

[ "$verbose" = 1 ] && export PHI512_VERBOSE=1
exec env LD_PRELOAD="$lib${LD_PRELOAD:+:$LD_PRELOAD}" "$@"
