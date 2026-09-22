# phi-env.sh: the one place a script learns which card it is talking to
# and what that card owns on the host. Sourced, not run:
#
#   . "$root/scripts/phi-env.sh"          # after $root is set
#   phi_env "$@"                          # eats a leading -c N / --card N
#   set -- "${PHI_ARGS[@]}"               # the remaining arguments
#
# Afterwards these are set (card 0 keeps the single-card names, exactly as
# host/crates/phi-vfio/src/cards.rs derives them):
#
#   PHI_CARD      the index, 0 to 15
#   PHI_BDF_SEL   its PCI address ("" when not on the bus)
#   PHI_PRESENT   yes or no
#   PHI_SOCK      control socket:   <runtime>/phictl/control.sock or .../phictl/N/control.sock
#   PHI_RUNDIR    the socket's directory (console.log and boot.pid live there)
#   PHI_PORT      SSH forward port: 2222 + N
#   PHI_HOSTMEM   host memory to give it, from ~/.config/phi/cards, else 6G
#   PHI_DISK_SEL  disk image from ~/.config/phi/cards (or $PHI_DISK for card 0), else ""
#   PHI_HOSTMEM_FILE  /dev/shm/phi-hostmem or /dev/shm/phi-hostmem-N
#   PHI_HOST_IP / PHI_CARD_IP   10.9.N.1 / 10.9.N.2
#   PHI_HOSTNAME  phi or phiN
#   PHI_UNIT      phi@N.service
#
# The card list itself comes from `phictl cards --plain`, so the scripts
# and the Rust tools can never disagree about which card is index 0.
# See phi-env.md.

phi_env() {
    PHI_ARGS=()
    local card="${PHI_CARD:-}"
    # Whether a card was named at all (option or variable), for commands
    # that act on every card when none is.
    PHI_CARD_GIVEN=${PHI_CARD:+yes}
    PHI_CARD_GIVEN=${PHI_CARD_GIVEN:-no}
    # Only LEADING card options are ours. Parsing the whole line ate the
    # -c of `phi -c 1 run sh -c '...'` and handed the shell's command text
    # to the index check.
    while [ $# -gt 0 ]; do
        case "$1" in
            -c|--card) card="$2"; PHI_CARD_GIVEN=yes; shift 2 ;;
            -c[0-9]*) card="${1#-c}"; PHI_CARD_GIVEN=yes; shift ;;
            --card=*) card="${1#--card=}"; PHI_CARD_GIVEN=yes; shift ;;
            *) break ;;
        esac
    done
    # Every card this host knows, for callers that loop over them.
    PHI_CARDS=$("${PHICTL:-$root/host/target/debug/phictl}" cards --plain 2>/dev/null | awk '$3 == "yes" {print $1}' | tr '\n' ' ')
    PHI_ARGS=("$@")
    [ -n "$card" ] || card=0
    case "$card" in
        ''|*[!0-9]*) echo "phi: card index must be a number, not '$card'" >&2; return 2 ;;
    esac
    if [ "$card" -ge 16 ]; then
        echo "phi: card index $card is out of range (0 to 15)" >&2
        return 2
    fi
    PHI_CARD=$card
    export PHI_CARD

    local P="${PHICTL:-$root/host/target/debug/phictl}"
    local line=""
    if [ -x "$P" ]; then
        line=$("$P" cards --plain 2>/dev/null | awk -v i="$card" '$1 == i {print; exit}')
    fi
    PHI_BDF_SEL=""; PHI_PRESENT=no; PHI_DISK_SEL=""; PHI_HOSTMEM=""
    if [ -n "$line" ]; then
        set -- $line
        PHI_BDF_SEL=$2
        PHI_PRESENT=$3
        [ "$4" != "-" ] && PHI_DISK_SEL=$4
        [ "$5" != "-" ] && PHI_HOSTMEM=$5
    fi
    # Card 0 honours the old single-card variables too.
    if [ "$card" = 0 ]; then
        [ -n "$PHI_DISK_SEL" ] || PHI_DISK_SEL="${PHI_DISK:-}"
        [ -n "$PHI_HOSTMEM" ] || PHI_HOSTMEM="${PHI_HOST_MEM:-}"
    fi
    [ -n "$PHI_HOSTMEM" ] || PHI_HOSTMEM=6G

    local rt="${XDG_RUNTIME_DIR:-/tmp/phictl-$(id -u)}/phictl"
    if [ "$card" = 0 ]; then
        PHI_RUNDIR="$rt"
        PHI_HOSTMEM_FILE=/dev/shm/phi-hostmem
        PHI_HOSTNAME=phi
    else
        PHI_RUNDIR="$rt/$card"
        PHI_HOSTMEM_FILE="/dev/shm/phi-hostmem-$card"
        PHI_HOSTNAME="phi$card"
    fi
    PHI_SOCK="$PHI_RUNDIR/control.sock"
    PHI_PORT=$((2222 + card))
    PHI_HOST_IP="10.9.$card.1"
    PHI_CARD_IP="10.9.$card.2"
    PHI_UNIT="phi@$card.service"
    return 0
}
