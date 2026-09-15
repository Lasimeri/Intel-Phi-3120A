#!/usr/bin/env bash
# phi-up.sh: boot the card in the background from an unprivileged shell and
# leave a control socket for phictl exec/put/get/status. No sudo: the phi
# group grants the VFIO device, the socket lives in the user's runtime
# directory. Usage: scripts/phi-up.sh [--toolchain] [extra phictl boot args]
#   --toolchain   also load the native clang (card/userland/components/clang-push.sh)
# Console output goes to $XDG_RUNTIME_DIR/phictl/console.log. See phi-up.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
P="$root/host/target/debug/phictl"
dir="${XDG_RUNTIME_DIR:-/tmp/phictl-$(id -u)}/phictl"
sock="$dir/control.sock"
log="$dir/console.log"
pidfile="$dir/boot.pid"
kernel="$root/card/kernel/build/out/arch/x86/boot/bzImage"
initrd="$root/card/initramfs/build/initramfs.cpio.gz"
toolchain=0; ssh=0
while [ $# -gt 0 ]; do
    case "$1" in
        --toolchain) toolchain=1; shift ;;
        --ssh) ssh=1; shift ;;
        *) break ;;
    esac
done
forward=(); [ "$ssh" = 1 ] && forward=(--forward 2222:22)
[ -x "$P" ] || { echo "phi-up.sh: build the host tools first (cd host && cargo build)" >&2; exit 1; }
[ -s "$kernel" ] && [ -s "$initrd" ] || { echo "phi-up.sh: kernel or initramfs missing" >&2; exit 1; }
id -nG | tr ' ' '\n' | grep -qx phi || { echo "phi-up.sh: not in group phi (sudo scripts/setup-arch.sh, then log in again)" >&2; exit 1; }
if pgrep -f "^(sudo )?\S*phictl boot " > /dev/null; then
    echo "phi-up.sh: a phictl boot is already running (pid $(pgrep -f "^(sudo )?\S*phictl boot " | head -1)); scripts/phi-down.sh first" >&2
    exit 1
fi
mkdir -p "$dir"; chmod 700 "$dir"
: > "$log"
PHICTL_SOCKET="$sock" setsid nohup "$P" boot --kernel "$kernel" --initrd "$initrd" \
    --cmdline "earlyprintk=phiring console=ttyPHI0" --serve "$sock" "${forward[@]}" "$@" < /dev/null > "$log" 2>&1 &
echo $! > "$pidfile"
echo "phi-up.sh: booting (pid $(cat "$pidfile")), console in $log"
# The agent answers once init has started: about 15 s from a cold open.
for i in $(seq 1 90); do
    if PHICTL_SOCKET="$sock" "$P" status > /dev/null 2>&1; then
        PHICTL_SOCKET="$sock" "$P" status
        if [ "$toolchain" = 1 ]; then
            PHICTL_SOCKET="$sock" bash "$root/card/userland/components/clang-push.sh"
        fi
        echo "phi-up.sh: ready; use: PHICTL_SOCKET=$sock $P exec -- CMD (or scripts/phi-run.sh CMD)"
        [ "$ssh" = 1 ] && echo "phi-up.sh: SSH: ssh -p 2222 root@localhost (forwarded through the ring, no root)"
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
