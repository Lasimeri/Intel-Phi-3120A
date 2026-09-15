#!/usr/bin/env bash
# phi-down.sh: halt the card started by phi-up.sh and release it. The
# agent gets a poweroff (POST "KH"); ending the phictl process then resets
# the card through VFIO. See phi-down.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
P="$root/host/target/debug/phictl"
dir="${XDG_RUNTIME_DIR:-/tmp/phictl-$(id -u)}/phictl"
sock="$dir/control.sock"
pidfile="$dir/boot.pid"
if [ -S "$sock" ]; then
    PHICTL_SOCKET="$sock" timeout 5 "$P" exec -- sh -c 'poweroff -f > /dev/null 2>&1 &' > /dev/null 2>&1 || true
    sleep 1
fi
if [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
    kill "$(cat "$pidfile")"
    sleep 1
    kill -9 "$(cat "$pidfile")" 2>/dev/null || true
fi
rm -f "$pidfile" "$sock"
pgrep -f "phictl boot" > /dev/null && echo "phi-down.sh: another phictl boot (not ours) is still running" >&2
echo "phi-down.sh: card released"
