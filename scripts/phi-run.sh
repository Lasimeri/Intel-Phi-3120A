#!/usr/bin/env bash
# phi-run.sh: run a command on a card started by phi-up.sh, from the
# user's shell. Usage: scripts/phi-run.sh [-c N] CMD [ARGS...]   (stdin is relayed)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
. "$root/scripts/phi-env.sh"
phi_env "$@"
set -- "${PHI_ARGS[@]}"
exec env PHICTL_SOCKET="$PHI_SOCK" "$root/host/target/debug/phictl" exec -- "$@"
