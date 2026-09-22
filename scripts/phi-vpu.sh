#!/usr/bin/env bash
# phi-vpu.sh: put the AVX-512 co-processor worker on a card and drive it.
#
#   scripts/phi-vpu.sh [-c N] deploy        copy the sources to the card and build there
#   scripts/phi-vpu.sh [-c N] start [T]     start the worker with T threads (default 57);
#                                           PHI_VPU_ARGS="-s MS -i US" passes worker options
#   scripts/phi-vpu.sh [-c N] stop
#   scripts/phi-vpu.sh [-c N] status        worker process on the card, control words on the host
#   scripts/phi-vpu.sh [-c N] log           the worker's output
#   scripts/phi-vpu.sh [-c N] poly [args]   run the host driver; deploys and starts first if needed
#
# N is the card index (default $PHI_CARD, else 0). Each card has its own
# host-memory window, so each runs its own worker. Needs the card up
# (phi -c N status). See phi-vpu.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
. "$root/scripts/phi-env.sh"
phi_env "$@"
set -- "${PHI_ARGS[@]}"
dir=${PHI_VPU_DIR:-/opt/phi-vpu}
threads_default=57

# The card over its own SSH forward, with the pinned host key: every card
# boots the same image and presents the same key, so one alias covers all.
ssh_() {
    ssh -o BatchMode=yes -o ConnectTimeout=10 -p "$PHI_PORT" -o IdentitiesOnly=yes -i "$HOME/.ssh/phi_ed25519" \
        -o UserKnownHostsFile="$HOME/.ssh/known_hosts_phi" -o HostKeyAlias=phi -o StrictHostKeyChecking=accept-new \
        root@127.0.0.1 "$@"
}
scp_() {
    scp -O -q -P "$PHI_PORT" -o IdentitiesOnly=yes -i "$HOME/.ssh/phi_ed25519" \
        -o UserKnownHostsFile="$HOME/.ssh/known_hosts_phi" -o HostKeyAlias=phi -o StrictHostKeyChecking=accept-new "$@"
}

# The worker's name inside a bracket class, so that pgrep -f over ssh does
# not match the ssh command line that carries the pattern itself. A plain
# `pkill -f phi-vpu-worker` kills the ssh session it is typed into.
pat='phi-vpu-worke[r]'

running() { ssh_ "pgrep -f '$pat' >/dev/null"; }

driver() {
    for cand in "$root/host/target/release/phi-vpu" "$root/host/target/debug/phi-vpu"; do
        [ -x "$cand" ] && { echo "$cand"; return; }
    done
    (cd "$root/host" && cargo build -q -p phi-vpu --release) >&2
    echo "$root/host/target/release/phi-vpu"
}

# The window the worker uses is the same memory that backs /dev/phiblk1,
# which the card can also be using as swap. Offloading over live swap
# would corrupt whichever side wrote second.
refuse_if_swapping() {
    if ssh_ 'grep -q phiblk1 /proc/swaps'; then
        echo "phi-vpu.sh: /dev/phiblk1 is a swap device on card $PHI_CARD; run: phi -c $PHI_CARD run swapoff /dev/phiblk1" >&2
        exit 1
    fi
}

cmd=${1:-}; shift || true
case "$cmd" in
    deploy)
        ssh_ "mkdir -p '$dir'"
        scp_ "$root/card/vpu/vpu_proto.h" "$root/card/vpu/vpu_worker.c" "$root/card/vpu/build.sh" \
            "$root/card/examples/avx512_poly.S" "root@127.0.0.1:$dir/"
        # A non-login shell over ssh has no /opt/phi/bin on PATH until the
        # card's next boot links the toolchain into /usr/bin; name it.
        ssh_ "cd '$dir' && PATH=/opt/phi/bin:\$PATH sh build.sh"
        ;;
    start)
        refuse_if_swapping
        n=${1:-$threads_default}
        if running; then ssh_ "pkill -f '$pat'"; sleep 0.5; fi
        # PHI_VPU_ARGS carries extra worker options (-s MS, -i US). The
        # kill above is a separate ssh call on purpose: a pkill in the same
        # command line as "./phi-vpu-worker" matches its own shell.
        ssh_ "cd '$dir' && setsid ./phi-vpu-worker -v ${PHI_VPU_ARGS:-} $n > worker.log 2>&1 < /dev/null & sleep 1"
        if running; then
            echo "worker started on card $PHI_CARD with $n threads; log: $dir/worker.log on the card"
        else
            echo "phi-vpu.sh: the worker did not stay up:" >&2
            ssh_ "cat '$dir/worker.log'" >&2
            exit 1
        fi
        ;;
    stop)
        if running; then ssh_ "pkill -f '$pat'"; echo "worker stopped on card $PHI_CARD"; else echo "no worker running on card $PHI_CARD"; fi
        ;;
    status)
        if running; then echo "card $PHI_CARD: worker running"; else echo "card $PHI_CARD: no worker"; fi
        "$(driver)" --card "$PHI_CARD" status
        ;;
    log)
        ssh_ "cat '$dir/worker.log'"
        ;;
    poly)
        if ! running; then
            ssh_ "test -x '$dir/phi-vpu-worker'" || "$0" -c "$PHI_CARD" deploy
            "$0" -c "$PHI_CARD" start
        fi
        exec "$(driver)" --card "$PHI_CARD" poly "$@"
        ;;
    *)
        sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
        exit 2
        ;;
esac
