#!/usr/bin/env bash
# phi.sh: one entry point for the Xeon Phi 3120A, for a person at the host's
# terminal and for anything scripting it. Everything below works from an
# unprivileged shell in the phi group; nothing here needs sudo.
#
# Symlinked as ~/.local/bin/phi by `phi.sh install-cli`. See phi.md.
set -euo pipefail

here=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
root=$(cd "$here/.." && pwd)
P="$root/host/target/debug/phictl"
TOP="$root/host/target/debug/phitop"
unit=phi.service

# The control socket, in the order a caller would expect: an explicit
# override, this session's runtime directory, the runtime directory of the
# user who owns the daemon, then the root boot's path.
resolve_sock() {
    local c
    for c in ${PHICTL_SOCKET:+"$PHICTL_SOCKET"} \
             "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/phictl/control.sock" \
             "/run/user/$(id -u)/phictl/control.sock" \
             "/run/phictl/control.sock"; do
        [ -S "$c" ] && { printf '%s\n' "$c"; return 0; }
    done
    # Nothing live: name where an unprivileged boot would put it, so error
    # messages point somewhere useful.
    printf '%s\n' "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/phictl/control.sock"
}
sock=$(resolve_sock)
rundir=$(dirname "$sock")

have_unit() { systemctl --user cat "$unit" > /dev/null 2>&1; }
unit_active() { [ "$(systemctl --user is-active "$unit" 2>/dev/null)" = active ]; }
card_up() { PHICTL_SOCKET="$sock" "$P" status > /dev/null 2>&1; }

need_card() {
    card_up && return 0
    echo "phi: the card is not up (no agent on $sock)." >&2
    if have_unit; then
        echo "     start it with: phi up        (systemd user unit $unit)" >&2
    else
        echo "     start it with: phi up        (scripts/phi-up.sh)" >&2
    fi
    exit 1
}

wait_for_agent() {
    local n=${1:-90}
    for ((i = 0; i < n; i++)); do
        card_up && return 0
        sleep 1
    done
    return 1
}

usage() {
    cat <<'USAGE'
phi: drive the Intel Xeon Phi 3120A from this host.

  phi up [ARGS...]      boot the card and serve it (systemd unit if installed)
  phi down              power the card off and release it
  phi restart           down then up
  phi status            unit, card, storage, and who can reach it
  phi run CMD [ARGS]    run a command on the card (stdin/stdout/stderr/exit relayed)
  phi sh                interactive shell on the card (pty, through the loopback forward)
  phi put SRC DST       copy a file to the card
  phi get SRC DST       copy a file from the card
  phi top [ARGS]        live viewer: per-thread load, temps, memory, PCIe rates
  phi sensors           temperatures, core voltage and clock (works while the card kernel is down)
  phi traffic           PCIe bytes moved, by path and direction
  phi console           follow the card's console
  phi log [ARGS]        the daemon's journal (systemd unit) or its console log
  phi disk ARGS         manage the disk image (create, check, usage)
  phi install-cli       symlink ~/.local/bin/phi and install fish completions
  phi help              this

Anything after `up` is passed to `phictl boot`, so `phi up --no-dma` works.
USAGE
}

cmd=${1:-help}
[ $# -gt 0 ] && shift

case "$cmd" in
up)
    if card_up; then
        echo "phi: already up ($sock)"
        exit 0
    fi
    if have_unit && [ $# -eq 0 ]; then
        systemctl --user start "$unit"
        echo "phi: $unit started; waiting for the card agent"
        wait_for_agent 90 || { echo "phi: no agent after 90 s; phi log" >&2; exit 1; }
        sock=$(resolve_sock)
    else
        [ $# -gt 0 ] && have_unit && unit_active && {
            echo "phi: $unit holds the card; stop it first (phi down)" >&2
            exit 1
        }
        "$root/scripts/phi-up.sh" "$@"
        sock=$(resolve_sock)
    fi
    PHICTL_SOCKET="$sock" "$P" status
    ;;

down)
    if have_unit && unit_active; then
        systemctl --user stop "$unit"
        echo "phi: $unit stopped"
    else
        "$root/scripts/phi-down.sh"
    fi
    ;;

restart)
    "$0" down || true
    sleep 1
    exec "$0" up "$@"
    ;;

status)
    if have_unit; then
        printf 'unit      %s (%s)\n' "$(systemctl --user is-active "$unit")" \
            "$(systemctl --user is-enabled "$unit" 2>/dev/null || echo 'not enabled')"
        # Lingering is the difference between "up at boot" and "up at login":
        # with it, logind starts this user's manager before anyone logs in.
        if [ "$(loginctl show-user "$(id -un)" -p Linger --value 2>/dev/null)" = yes ]; then
            printf 'starts    at host boot, runs with nobody logged in (linger on)\n'
        else
            printf 'starts    at the first login, stops with the last session (phi-autoboot.sh at-boot)\n'
        fi
    else
        printf 'unit      not installed (scripts/phi-autoboot.sh install)\n'
    fi
    printf 'socket    %s\n' "$sock"
    if card_up; then
        printf 'agent     %s\n' "$(PHICTL_SOCKET="$sock" "$P" status)"
        PHICTL_SOCKET="$sock" "$P" exec -- sh -c '
            printf "kernel    %s\n" "$(uname -sr)"
            printf "cpus      %s online\n" "$(nproc)"
            printf "uptime    %s\n" "$(uptime | sed "s/^ *//")"
            awk "/MemTotal|MemAvailable|SwapTotal|SwapFree/ {v[\$1]=\$2/1024}
                 END {printf \"memory    %d MiB free of %d MiB\\n\", v[\"MemAvailable:\"], v[\"MemTotal:\"];
                      printf \"swap      %d MiB free of %d MiB (host RAM)\\n\", v[\"SwapFree:\"], v[\"SwapTotal:\"]}" /proc/meminfo
            df -h /data 2>/dev/null | awk "NR==2 {printf \"/data     %s free of %s\\n\", \$4, \$2}"
        ' 2>/dev/null || true
    else
        printf 'agent     not reachable (card down)\n'
    fi
    echo
    echo 'reachable from:'
    printf '  %-30s %s\n' "$sock" 'control socket, this user only (0600 in a 0700 dir)'
    if ss -ltn 2>/dev/null | grep -q '127.0.0.1:2222'; then
        printf '  %-30s %s\n' '127.0.0.1:2222' 'SSH, loopback only (forwarder inside the daemon)'
    fi
    if ip -o link show phi0 > /dev/null 2>&1 || ip -o link show type bridge 2>/dev/null | grep -q phi; then
        printf '  %-30s %s\n' 'phi0 (host TAP)' 'a host TAP exists: the card is on a host network'
    fi
    ss -ltn 2>/dev/null | awk '$4 !~ /^127\.0\.0\.1:|^\[::1\]:/ && $4 ~ /:2222$/ {
        print "  WARNING: SSH forward is bound beyond loopback: " $4 }'
    ;;

run|exec)
    [ $# -gt 0 ] || { echo "phi run: need a command" >&2; exit 2; }
    need_card
    exec env PHICTL_SOCKET="$sock" "$P" exec -- "$@"
    ;;

sh|shell)
    need_card
    ss -ltn 2>/dev/null | grep -q '127.0.0.1:2222' || {
        echo "phi sh: no SSH forward on 127.0.0.1:2222." >&2
        echo "        The daemon needs --forward 2222:22 (the unit passes it;" >&2
        echo "        phi-up.sh needs --ssh). Meanwhile: phi run CMD" >&2
        exit 1
    }
    # With no arguments this is an interactive login shell, which reads
    # /etc/profile on the card. With arguments, wrap them in `sh -lc` so they
    # see the same PATH: ssh runs a plain non-login shell otherwise, and
    # /opt/phi/bin would be missing.
    sshopts=(-p 2222 -o IdentitiesOnly=yes -i "$HOME/.ssh/phi_ed25519"
             -o UserKnownHostsFile="$HOME/.ssh/known_hosts_phi"
             -o StrictHostKeyChecking=accept-new)
    if [ $# -eq 0 ]; then
        exec ssh "${sshopts[@]}" root@127.0.0.1
    fi
    # Single-quote the command so the outer non-login shell dropbear starts
    # passes it through untouched; only the login shell expands it.
    q=$(printf "%s" "$*" | sed "s/'/'\\\\''/g")
    exec ssh "${sshopts[@]}" root@127.0.0.1 "sh -lc '$q'"
    ;;

put)
    [ $# -eq 2 ] || { echo "phi put: need SRC DST" >&2; exit 2; }
    need_card
    exec env PHICTL_SOCKET="$sock" "$P" put "$@"
    ;;

get)
    [ $# -eq 2 ] || { echo "phi get: need SRC DST" >&2; exit 2; }
    need_card
    exec env PHICTL_SOCKET="$sock" "$P" get "$@"
    ;;

top)
    need_card
    exec env PHICTL_SOCKET="$sock" "$TOP" "$@"
    ;;

sensors|traffic)
    # Answered by the daemon itself, so they work while the card's kernel is
    # booting, hung or halted. They still need the daemon: it is the process
    # holding the VFIO device.
    [ -S "$sock" ] || {
        echo "phi $cmd: no daemon on $sock. Start one with: phi up" >&2
        exit 1
    }
    exec env PHICTL_SOCKET="$sock" "$P" "$cmd" "$@"
    ;;

console)
    if [ -f "$rundir/console.log" ]; then
        exec tail -n "${1:-40}" -f "$rundir/console.log"
    elif have_unit; then
        exec journalctl --user -u "$unit" -f -n "${1:-40}"
    else
        echo "phi console: no console log at $rundir/console.log and no $unit" >&2
        exit 1
    fi
    ;;

log)
    if have_unit; then
        exec journalctl --user -u "$unit" "$@"
    else
        exec less "$rundir/console.log"
    fi
    ;;

disk)
    exec "$root/scripts/phi-disk.sh" "$@"
    ;;

install-cli)
    mkdir -p "$HOME/.local/bin"
    ln -sfn "$root/scripts/phi.sh" "$HOME/.local/bin/phi"
    echo "installed $HOME/.local/bin/phi -> $root/scripts/phi.sh"
    if [ -d "$HOME/.config/fish" ]; then
        mkdir -p "$HOME/.config/fish/completions"
        # Symlinked, not copied, so an edit in the repository takes effect
        # without reinstalling.
        ln -sfn "$root/scripts/phi.fish" "$HOME/.config/fish/completions/phi.fish"
        echo "installed $HOME/.config/fish/completions/phi.fish"
    fi
    case ":$PATH:" in
        *":$HOME/.local/bin:"*) ;;
        *) echo "note: $HOME/.local/bin is not on PATH in this shell" >&2 ;;
    esac
    ;;

help|-h|--help)
    usage
    ;;

*)
    echo "phi: unknown command '$cmd'" >&2
    usage >&2
    exit 2
    ;;
esac
