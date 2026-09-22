#!/usr/bin/env bash
# phi.sh: one entry point for the Xeon Phi cards, for a person at the host's
# terminal and for anything scripting them. Everything below works from an
# unprivileged shell in the phi group; nothing here needs sudo.
#
#   phi [-c N] COMMAND ...      N selects a card (default $PHI_CARD, else 0)
#
# Symlinked as ~/.local/bin/phi by `phi.sh install-cli`. See phi.md.
set -euo pipefail

here=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
root=$(cd "$here/.." && pwd)
P="$root/host/target/debug/phictl"
TOP="$root/host/target/debug/phitop"
. "$root/scripts/phi-env.sh"
phi_env "$@"
set -- "${PHI_ARGS[@]}"
unit=$PHI_UNIT

# The control socket, in the order a caller would expect: an explicit
# override, this card's socket in the runtime directory, then the root
# daemon's path for this card.
resolve_sock() {
    local c
    for c in ${PHICTL_SOCKET:+"$PHICTL_SOCKET"} "$PHI_SOCK" \
             "/run/user/$(id -u)/phictl${PHI_CARD:+/$PHI_CARD}/control.sock" \
             "/run/phictl${PHI_CARD:+/$PHI_CARD}/control.sock"; do
        case "$c" in */0/control.sock) c="${c%/0/control.sock}/control.sock" ;; esac
        [ -S "$c" ] && { printf '%s\n' "$c"; return 0; }
    done
    # Nothing live: name where an unprivileged boot would put it, so error
    # messages point somewhere useful.
    printf '%s\n' "$PHI_SOCK"
}
sock=$(resolve_sock)
rundir=$(dirname "$sock")

have_unit() { systemctl --user cat "$unit" > /dev/null 2>&1; }
unit_active() { [ "$(systemctl --user is-active "$unit" 2>/dev/null)" = active ]; }
card_up() { PHICTL_SOCKET="$sock" "$P" status > /dev/null 2>&1; }

need_card() {
    card_up && return 0
    echo "phi: card $PHI_CARD is not up (no agent on $sock)." >&2
    if have_unit; then
        echo "     start it with: phi -c $PHI_CARD up        (systemd user unit $unit)" >&2
    else
        echo "     start it with: phi -c $PHI_CARD up        (scripts/phi-up.sh)" >&2
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
phi: drive the Intel Xeon Phi cards from this host.

  phi [-c N] COMMAND        N is the card index, 0 to 15 (default $PHI_CARD, else 0)

  phi cards               every card this host knows: index, address, link, state
  phi cards init [DIR]    write ~/.config/phi/cards from the enumerated cards
  phi up [all|args]       boot the card and serve it (the systemd unit if installed,
                          else scripts/phi-up.sh; args go to phi-up.sh); `all` = every card
  phi down [all]          power the card off and release it
  phi restart
  phi status              unit, card, storage, and every way in; with no -c and
                          several cards, one line per card
  phi vpu ARGS            the AVX-512 co-processor worker on this card (scripts/phi-vpu.sh)
  phi ssh-config [--apply] a ~/.ssh/config stanza per card: ssh phi, ssh phi1, ...
  phi run CMD [ARGS]      run a command on the card (control socket; stdin relayed)
  phi sh [CMD]            interactive shell on the card, or one command, over SSH
  phi put SRC DST         copy a file to the card
  phi get SRC DST         copy a file from the card
  phi top [args]          live viewer (phitop); -b N for plain frames
  phi sensors             die temperatures, core voltage, clock (works while booting)
  phi traffic             PCIe bytes by path and direction
  phi console [N]         follow the card's console (last N lines first)
  phi log [args]          the daemon's journal (systemd) or console log
  phi disk ARGS           manage a disk image (create, check, usage)
  phi install-cli         put phi, phictl and phitop on PATH
USAGE
}

cmd=${1:-help}
[ $# -gt 0 ] && shift

# `phi up all`, `phi down all`, `phi status` with no card named and more
# than one present: do it per card, through this same script.
each_card() {
    local c
    for c in $PHI_CARDS; do
        echo "== card $c"
        "$0" -c "$c" "$@" || true
    done
}
n_cards=$(printf '%s' "$PHI_CARDS" | wc -w)

case "$cmd" in
cards)
    if [ "${1:-}" = init ]; then
        cfg="${XDG_CONFIG_HOME:-$HOME/.config}/phi/cards"
        [ -e "$cfg" ] && { echo "phi cards init: $cfg exists; edit it instead" >&2; exit 1; }
        dir="${2:-${PHI_DISK:+$(dirname "$PHI_DISK")}}"
        mkdir -p "$(dirname "$cfg")"
        {
            echo "# Xeon Phi cards on this host, one per line; the line order is the card"
            echo "# index. Columns: BDF DISK HOSTMEM (written by phi cards init, $(date +%F))"
            i=0
            while read -r b; do
                disk="-"
                if [ -n "$dir" ]; then
                    if [ "$i" = 0 ]; then disk="${PHI_DISK:-$dir/disk.img}"; else disk="$dir/disk$i.img"; fi
                fi
                printf '%-14s %s %s\n' "$b" "$disk" "${PHI_HOST_MEM:-6G}"
                i=$((i + 1))
            done < <(lspci -Dn -d 8086:225d | awk '{print $1}' | sort)
        } > "$cfg"
        echo "wrote $cfg:"; cat "$cfg"
        exit 0
    fi
    "$P" cards
    if systemctl --user cat phi@.service > /dev/null 2>&1; then
        printf '\nunits: '
        systemctl --user list-units 'phi@*' --all --no-legend --plain 2>/dev/null | awk '{printf "%s %s  ", $1, $3}'
        echo
    fi
    ;;

up)
    if [ "${1:-}" = all ]; then shift; each_card up "$@"; exit 0; fi
    if card_up; then
        echo "phi: card $PHI_CARD is already up ($sock)"
        exit 0
    fi
    if have_unit && [ $# -eq 0 ]; then
        systemctl --user start "$unit"
        echo "phi: $unit started; waiting for the card agent"
        wait_for_agent 90 || { echo "phi: no agent after 90 s; phi -c $PHI_CARD log" >&2; exit 1; }
        sock=$(resolve_sock)
    else
        [ $# -gt 0 ] && have_unit && unit_active && {
            echo "phi: $unit holds the card; stop it first (phi -c $PHI_CARD down)" >&2
            exit 1
        }
        "$root/scripts/phi-up.sh" -c "$PHI_CARD" "$@"
        sock=$(resolve_sock)
    fi
    PHICTL_SOCKET="$sock" "$P" status
    ;;

down)
    if [ "${1:-}" = all ]; then each_card down; exit 0; fi
    if have_unit && unit_active; then
        systemctl --user stop "$unit"
        echo "phi: $unit stopped"
    else
        "$root/scripts/phi-down.sh" -c "$PHI_CARD"
    fi
    ;;

restart)
    "$0" -c "$PHI_CARD" down || true
    sleep 1
    exec "$0" -c "$PHI_CARD" up "$@"
    ;;

status)
    if [ "$PHI_CARD_GIVEN" = no ] && [ "$n_cards" -gt 1 ] && [ "${1:-}" != one ]; then
        # No card named and several present: the overview, then one line
        # per card. `phi -c N status` has the full picture for one.
        "$P" cards
        echo
        for c in $PHI_CARDS; do
            s=$("$0" -c "$c" status one 2>/dev/null | awk '/^(hostname|uptime|memory|\/data)/ {sub(/^[a-z\/]+ +/, ""); printf "%s | ", $0}')
            printf 'card %s  %s\n' "$c" "${s%| }"
        done
        echo
        echo "one card in full: phi -c N status"
        exit 0
    fi
    printf 'card      %s (%s%s)\n' "$PHI_CARD" "${PHI_BDF_SEL:-not enumerated}" "${PHI_PRESENT/yes/}"
    if have_unit; then
        printf 'unit      %s %s (%s)\n' "$unit" "$(systemctl --user is-active "$unit")" \
            "$(systemctl --user is-enabled "$unit" 2>/dev/null || echo 'not enabled')"
        # Lingering is the difference between "up at boot" and "up at login":
        # with it, logind starts this user's manager before anyone logs in.
        if [ "$(loginctl show-user "$(id -un)" -p Linger --value 2>/dev/null)" = yes ]; then
            printf 'starts    at host boot, runs with nobody logged in (linger on)\n'
        else
            printf 'starts    at the first login, stops with the last session (phi-autoboot.sh at-boot)\n'
        fi
    else
        printf 'unit      not installed (scripts/phi-autoboot.sh install %s)\n' "$PHI_CARD"
    fi
    printf 'socket    %s\n' "$sock"
    if card_up; then
        printf 'agent     %s\n' "$(PHICTL_SOCKET="$sock" "$P" status)"
        PHICTL_SOCKET="$sock" "$P" exec -- sh -c '
            printf "hostname  %s\n" "$(hostname)"
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
    # Name the command, not just the address. A bare "127.0.0.1:2222" reads
    # like something you can hand to ssh, and ssh does not take host:port.
    echo 'ways in (all of them local to this host):'
    printf '  %-22s %s\n' "phi -c $PHI_CARD run CMD" "control socket $sock"
    printf '  %-22s %s\n' '' 'mode 0600 in a 0700 dir, SO_PEERCRED per connection'
    if ss -ltn 2>/dev/null | grep -q "127.0.0.1:$PHI_PORT"; then
        printf '  %-22s %s\n' "phi -c $PHI_CARD sh" "SSH with a pty, through the forwarder on 127.0.0.1:$PHI_PORT"
        if grep -qE "^Host .*\b$PHI_HOSTNAME\b" "$HOME/.ssh/config" 2>/dev/null; then
            printf '  %-22s %s\n' "ssh $PHI_HOSTNAME" 'the same, through your ~/.ssh/config stanza'
        else
            printf '  %-22s %s\n' "ssh -p $PHI_PORT ..." 'root@127.0.0.1; ssh takes -p, never host:port'
        fi
    fi
    if ip -o link show phi0 > /dev/null 2>&1 || ip -o link show type bridge 2>/dev/null | grep -q phi; then
        printf '  %-22s %s\n' 'phi0 (host TAP)' 'a host TAP exists: the card is on a host network'
    fi
    ss -ltn 2>/dev/null | awk -v p=":$PHI_PORT\$" '$4 !~ /^127\.0\.0\.1:|^\[::1\]:/ && $4 ~ p {
        print "  WARNING: SSH forward is bound beyond loopback: " $4 }'
    ;;

run|exec)
    [ $# -gt 0 ] || { echo "phi run: need a command" >&2; exit 2; }
    need_card
    exec env PHICTL_SOCKET="$sock" "$P" exec -- "$@"
    ;;

sh|shell)
    need_card
    ss -ltn 2>/dev/null | grep -q "127.0.0.1:$PHI_PORT" || {
        echo "phi sh: no SSH forward on 127.0.0.1:$PHI_PORT for card $PHI_CARD." >&2
        echo "        The daemon needs --forward $PHI_PORT:22 (phi-boot.sh passes it)." >&2
        echo "        Meanwhile: phi -c $PHI_CARD run CMD" >&2
        exit 1
    }
    # With no arguments this is an interactive login shell, which reads
    # /etc/profile on the card. With arguments, wrap them in `sh -lc` so they
    # see the same PATH: ssh runs a plain non-login shell otherwise, and
    # /opt/phi/bin would be missing.
    #
    # HostKeyAlias: every card boots the same image and therefore presents
    # the same host key, so one pinned entry ("phi") covers every port.
    sshopts=(-p "$PHI_PORT" -o IdentitiesOnly=yes -i "$HOME/.ssh/phi_ed25519"
             -o UserKnownHostsFile="$HOME/.ssh/known_hosts_phi"
             -o HostKeyAlias=phi
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
        echo "phi $cmd: no daemon on $sock. Start one with: phi -c $PHI_CARD up" >&2
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

vpu)
    # The AVX-512 co-processor worker on this card (scripts/phi-vpu.md).
    exec "$root/scripts/phi-vpu.sh" -c "$PHI_CARD" "$@"
    ;;

ssh-config)
    # One ~/.ssh/config stanza per known card: `ssh phiN` reaches card N
    # through its own forward with the one pinned host key. `phi` stays
    # an alias of card 0. Printed by default; --apply appends the stanzas
    # that are missing.
    key="$HOME/.ssh/phi_ed25519"
    out=""
    for c in $(awk '{print $1}' <<< "$("$P" cards --plain 2>/dev/null)"); do
        name=phi$c; [ "$c" = 0 ] && name="phi phi0"
        grep -qE "^Host .*\bphi$c\b|^Host .*\bphi\b" "$HOME/.ssh/config" 2>/dev/null && [ "$c" = 0 ] && continue
        grep -qE "^Host .*\bphi$c\b" "$HOME/.ssh/config" 2>/dev/null && continue
        out+="Host $name
    HostName 127.0.0.1
    Port $((2222 + c))
    User root
    IdentityFile $key
    IdentitiesOnly yes
    UserKnownHostsFile $HOME/.ssh/known_hosts_phi
    HostKeyAlias phi
    StrictHostKeyChecking accept-new

"
    done
    if [ -z "$out" ]; then echo "phi ssh-config: every known card already has a stanza"; exit 0; fi
    if [ "${1:-}" = --apply ]; then
        mkdir -p "$HOME/.ssh"; chmod 700 "$HOME/.ssh"
        printf '\n# Added by phi ssh-config (Intel Phi 3120A project)\n%s' "$out" >> "$HOME/.ssh/config"
        chmod 600 "$HOME/.ssh/config"
        echo "appended to $HOME/.ssh/config:"; printf '%s' "$out"
    else
        printf '%s' "$out"
        echo "# phi ssh-config --apply appends these to ~/.ssh/config"
    fi
    ;;

install-cli)
    mkdir -p "$HOME/.local/bin"
    ln -sfn "$root/scripts/phi.sh" "$HOME/.local/bin/phi"
    echo "installed $HOME/.local/bin/phi -> $root/scripts/phi.sh"
    # The binaries too, not only the wrapper. `phi top` and `phitop` should
    # both work: a tool you can name is a tool you can find, and a wrapper
    # that hides its own binaries is the reason `phitop` said "not found"
    # while `phi top` worked.
    for b in phitop phictl phi-isa-audit knc-mvex-gen; do
        if [ -x "$root/host/target/debug/$b" ]; then
            ln -sfn "$root/host/target/debug/$b" "$HOME/.local/bin/$b"
            echo "installed $HOME/.local/bin/$b"
        else
            echo "skipped $b (not built; make build)" >&2
        fi
    done
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
