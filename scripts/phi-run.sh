#!/usr/bin/env bash
# phi-run.sh: run a command on the card started by phi-up.sh, from the
# user's shell. Usage: scripts/phi-run.sh CMD [ARGS...]   (stdin is relayed)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
dir="${XDG_RUNTIME_DIR:-/tmp/phictl-$(id -u)}/phictl"
exec env PHICTL_SOCKET="$dir/control.sock" "$root/host/target/debug/phictl" exec -- "$@"
