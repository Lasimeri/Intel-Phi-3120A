#!/usr/bin/env bash
# phi512.sh: run a program that needs AVX-512 on a host that has none.
#
#   scripts/phi512.sh ./my-avx512-program [args...]
#
# The program is not modified, recompiled, or asked to cooperate. It
# executes an AVX-512 instruction, this host refuses it, and libphi512
# performs the instruction and lets the program continue.
#
#   --card N    the card to use (default: PHI512_CARD, else 0)
#   --emulate   run the AVX-512 in software on this host instead of on a card
#   --verbose   report every region the card ran, with its phases and times
#   --check     say whether this host needs the library at all, and exit
#
# The card's worker is started (and deployed) if it is not running.
#
# See phi512.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)

verbose=0
card=${PHI512_CARD:-0}
emulate=${PHI512_EMULATE:-}
while [ $# -gt 0 ]; do
    case "$1" in
        --verbose|-v) verbose=1; shift ;;
        --card|-c) card=$2; shift 2 ;;
        --card=*) card=${1#--card=}; shift ;;
        --emulate) emulate=1; shift ;;
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
# Where the library is. Three places, in order: an explicit override, the
# build tree when this script is being run from a clone, and the installed
# copy. The build tree wins during development so a fresh build is picked
# up without reinstalling.
lib="${PHI512_LIB:-}"

if [ -z "$lib" ] && [ -d "$root/host/target" ]; then
    # LD_PRELOAD splits its value on spaces and colons, and this repository
    # is normally cloned to a path with a space in it ("Intel Phi 3120A").
    # The space-free symlink that toolchain/env.sh maintains is the way in;
    # if it is missing, make one, because no quoting would help here.
    cache="${XDG_CACHE_HOME:-$HOME/.cache}/intel-phi-3120a"
    if [ ! -e "$cache" ]; then ln -sfn "$root" "$cache"; fi
    for cand in "$cache/host/target/release/libphi512.so" "$cache/host/target/debug/libphi512.so"; do
        [ -f "$cand" ] && { lib="$cand"; break; }
    done
fi
[ -n "$lib" ] || { [ -f /usr/lib/libphi512.so ] && lib=/usr/lib/libphi512.so; }
[ -n "$lib" ] || { echo "$0: libphi512.so not found; run 'make build' or scripts/phi512-install.sh" >&2; exit 1; }
[ -f "$lib" ] || { echo "$0: $lib is missing" >&2; exit 1; }
case "$lib" in *" "*) echo "$0: the library path contains a space, which LD_PRELOAD cannot express: $lib" >&2; exit 1 ;; esac

[ "$verbose" = 1 ] && export PHI512_VERBOSE=1
export PHI512_CARD="$card"
if [ -n "$emulate" ]; then
    export PHI512_EMULATE=1
else
    # The card executes the program's AVX-512: its worker must be polling.
    # A missing worker is started here, deployed first if the card has none.
    driver=""
    for cand in "$root/host/target/release/phi-vpu" "$root/host/target/debug/phi-vpu"; do
        [ -x "$cand" ] && { driver=$cand; break; }
    done
    if [ -z "$driver" ] || ! "$driver" --card "$card" status 2>/dev/null | grep -q "worker: polling"; then
        echo "phi512: card $card has no worker polling; starting it" >&2
        "$root/scripts/phi-vpu.sh" -c "$card" start >&2 || {
            echo "phi512: could not start the worker on card $card (is the card up? phi -c $card status)" >&2
            exit 1
        }
    fi
fi
exec env LD_PRELOAD="$lib${LD_PRELOAD:+:$LD_PRELOAD}" "$@"
