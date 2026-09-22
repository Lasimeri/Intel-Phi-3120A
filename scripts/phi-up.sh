#!/usr/bin/env bash
# phi-up.sh: boot a card in the background from an unprivileged shell and
# leave a control socket for phictl exec/put/get/status. No sudo: the phi
# group grants the VFIO device, the socket lives in the user's runtime
# directory. Usage: scripts/phi-up.sh [-c N] [--toolchain] [extra phictl boot args]
#   -c N          which card (default $PHI_CARD, else 0); see phictl cards
#   --toolchain   also load the native clang (card/userland/components/clang-push.sh)
#   --ssh         accepted for compatibility: every card gets its SSH forward
#                 (127.0.0.1:2222+N) now
#   --disk PATH   serve PATH as the card's persistent disk instead of the one
#                 in ~/.config/phi/cards (or $PHI_DISK for card 0)
#   --host-mem SZ / --no-host-mem   host RAM for the card (default 6G)
# PHI_CMDLINE_EXTRA is appended to the kernel command line (e.g. knc_blk.direct=1).
# Console output goes to the card's runtime directory as console.log. See phi-up.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
. "$root/scripts/phi-env.sh"
phi_env "$@"
set -- "${PHI_ARGS[@]}"

P="$root/host/target/debug/phictl"
toolchain=0
while [ $# -gt 0 ]; do
    case "$1" in
        --toolchain) toolchain=1; shift ;;
        --ssh) shift ;;
        --disk) PHI_DISK_SEL="$2"; shift 2 ;;
        --host-mem) PHI_HOSTMEM="$2"; shift 2 ;;
        --no-host-mem) PHI_HOSTMEM=none; shift ;;
        *) break ;;
    esac
done
export PHI_DISK_SEL PHI_HOSTMEM
[ -x "$P" ] || { echo "phi-up.sh: build the host tools first (cd host && cargo build)" >&2; exit 1; }
id -nG | tr ' ' '\n' | grep -qx phi || { echo "phi-up.sh: not in group phi (sudo scripts/setup-arch.sh, then log in again)" >&2; exit 1; }
[ "$PHI_PRESENT" = yes ] || { echo "phi-up.sh: card $PHI_CARD is not on the PCI bus (phictl cards)" >&2; exit 1; }
if [ -S "$PHI_SOCK" ] && PHICTL_SOCKET="$PHI_SOCK" "$P" status > /dev/null 2>&1; then
    echo "phi-up.sh: card $PHI_CARD is already up ($PHI_SOCK)" >&2
    exit 1
fi
# One daemon per card: the pattern names this card's address.
if pgrep -f "phictl.*--card $PHI_CARD .*boot |phictl.*--bdf $PHI_BDF_SEL .*boot " > /dev/null; then
    echo "phi-up.sh: a phictl boot for card $PHI_CARD is already running; scripts/phi-down.sh -c $PHI_CARD first" >&2
    exit 1
fi
mkdir -p "$PHI_RUNDIR"; chmod 700 "$PHI_RUNDIR"
log="$PHI_RUNDIR/console.log"
pidfile="$PHI_RUNDIR/boot.pid"
: > "$log"
# phi-boot.sh assembles the same command line the systemd unit uses, so a
# card booted here is identical to one booted at host boot.
PHI_DISK_SEL="$PHI_DISK_SEL" PHI_HOSTMEM="$PHI_HOSTMEM" setsid nohup "$root/scripts/phi-boot.sh" -c "$PHI_CARD" "$@" < /dev/null > "$log" 2>&1 &
echo $! > "$pidfile"
echo "phi-up.sh: booting card $PHI_CARD ($PHI_BDF_SEL, pid $(cat "$pidfile")), console in $log"
# The agent answers once init has started: about 15 s from a cold open.
for i in $(seq 1 90); do
    if PHICTL_SOCKET="$PHI_SOCK" "$P" status > /dev/null 2>&1; then
        PHICTL_SOCKET="$PHI_SOCK" "$P" status
        if [ "$toolchain" = 1 ]; then
            PHICTL_SOCKET="$PHI_SOCK" bash "$root/card/userland/components/clang-push.sh"
        fi
        echo "phi-up.sh: ready; use: phi -c $PHI_CARD run CMD, or PHICTL_SOCKET=$PHI_SOCK $P exec -- CMD"
        echo "phi-up.sh: SSH: ssh -p $PHI_PORT root@127.0.0.1 (forwarded through the ring, no root), or: phi -c $PHI_CARD sh"
        exit 0
    fi
    if ! kill -0 "$(cat "$pidfile")" 2>/dev/null; then
        echo "phi-up.sh: phictl boot exited; last lines of $log:" >&2
        tail -5 "$log" >&2
        exit 1
    fi
    sleep 1
done
echo "phi-up.sh: the agent did not answer within 90 s; last lines of $log:" >&2
tail -5 "$log" >&2
exit 1
