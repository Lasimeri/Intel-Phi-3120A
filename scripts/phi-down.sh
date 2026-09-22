#!/usr/bin/env bash
# phi-down.sh: halt a card started by phi-up.sh and release it. The agent
# gets a plain `poweroff`, which signals PID 1 so init can stop the services,
# swapoff and unmount /data before the kernel halts (POST "KH"); ending the
# phictl process then resets the card through VFIO.
#   scripts/phi-down.sh [-c N]      # default $PHI_CARD, else 0
# See phi-down.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
. "$root/scripts/phi-env.sh"
phi_env "$@"
P="$root/host/target/debug/phictl"
sock="$PHI_SOCK"
pidfile="$PHI_RUNDIR/boot.pid"
if [ -S "$sock" ]; then
    # Plain poweroff: busybox signals PID 1 (SIGUSR2) and init runs its
    # shutdown. `poweroff -f` would call reboot(2) straight from the shell and
    # leave the ext4 journal to replay on the next boot.
    PHICTL_SOCKET="$sock" timeout 5 "$P" exec -- sh -c 'poweroff > /dev/null 2>&1 &' > /dev/null 2>&1 || true
    # Give init time to unmount: killall, a 1 s grace, swapoff, sync, umount.
    for _ in 1 2 3 4 5 6 7 8; do
        PHICTL_SOCKET="$sock" timeout 2 "$P" status > /dev/null 2>&1 || break
        sleep 1
    done
fi
if [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
    # phi-up.sh recorded phi-boot.sh's pid, which exec'd into phictl.
    kill "$(cat "$pidfile")"
    sleep 1
    kill -9 "$(cat "$pidfile")" 2>/dev/null || true
fi
rm -f "$pidfile" "$sock"
echo "phi-down.sh: card $PHI_CARD released"
