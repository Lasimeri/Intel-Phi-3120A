#!/usr/bin/env bash
# phi-vpu.sh: put the AVX-512 co-processor worker on the card and drive it.
#
#   scripts/phi-vpu.sh deploy        copy the sources to the card and build there
#   scripts/phi-vpu.sh start [N]     start the worker with N threads (default 57)
#   scripts/phi-vpu.sh stop
#   scripts/phi-vpu.sh status        worker process on the card, control words on the host
#   scripts/phi-vpu.sh log           the worker's output
#   scripts/phi-vpu.sh poly [args]   run the host driver; deploys and starts first if needed
#
# Needs the card up (phi status) and reachable as `ssh phi`. See phi-vpu.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
host=${PHI_SSH_HOST:-phi}
dir=${PHI_VPU_DIR:-/opt/phi-vpu}
threads_default=57

ssh_() { ssh -o BatchMode=yes -o ConnectTimeout=10 "$host" "$@"; }

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
        echo "phi-vpu.sh: /dev/phiblk1 is a swap device on the card; run: ssh $host swapoff /dev/phiblk1" >&2
        exit 1
    fi
}

cmd=${1:-}; shift || true
case "$cmd" in
    deploy)
        ssh_ "mkdir -p '$dir'"
        scp -O -q "$root/card/vpu/vpu_proto.h" "$root/card/vpu/vpu_worker.c" "$root/card/vpu/build.sh" \
            "$root/card/examples/avx512_poly.S" "$host:$dir/"
        ssh_ "cd '$dir' && sh build.sh"
        ;;
    start)
        refuse_if_swapping
        n=${1:-$threads_default}
        if running; then ssh_ "pkill -f '$pat'"; sleep 0.5; fi
        # setsid plus a trailing sleep in the same command, or the process
        # dies with the ssh session.
        ssh_ "cd '$dir' && setsid ./phi-vpu-worker -v $n > worker.log 2>&1 < /dev/null & sleep 1"
        if running; then
            echo "worker started with $n threads; log: $dir/worker.log on the card"
        else
            echo "phi-vpu.sh: the worker did not stay up:" >&2
            ssh_ "cat '$dir/worker.log'" >&2
            exit 1
        fi
        ;;
    stop)
        if running; then ssh_ "pkill -f '$pat'"; echo "worker stopped"; else echo "no worker running"; fi
        ;;
    status)
        if running; then echo "card: worker running"; else echo "card: no worker"; fi
        "$(driver)" status
        ;;
    log)
        ssh_ "cat '$dir/worker.log'"
        ;;
    poly)
        if ! running; then
            ssh_ "test -x '$dir/phi-vpu-worker'" || "$0" deploy
            "$0" start
        fi
        exec "$(driver)" poly "$@"
        ;;
    *)
        sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
        exit 2
        ;;
esac
