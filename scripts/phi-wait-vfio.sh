#!/usr/bin/env bash
# phi-wait-vfio.sh: block until a card's VFIO group node exists and this
# process can open it, or fail after a timeout.
#
# Why it is a script and not an ExecStartPre one-liner: systemd expands `$`
# in Exec lines itself, so a shell snippet with `$(...)` and `$var` has to be
# escaped into illegibility. It also has to exit non-zero on failure, because
# a unit whose ConditionPathExists fails is marked *skipped*, and Restart=
# never acts on a skip. See phi-wait-vfio.md.
#
#   scripts/phi-wait-vfio.sh [-c N] [SECONDS]   # default 60; PHI_BDF overrides the card
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
. "$root/scripts/phi-env.sh"
phi_env "$@"
set -- "${PHI_ARGS[@]}"

timeout=${1:-60}
bdf=${PHI_BDF:-$PHI_BDF_SEL}
if [ -z "$bdf" ]; then
    # phictl may not be built yet on a fresh clone; fall back to the bus.
    bdf=$(lspci -Dn -d 8086:225d 2>/dev/null | awk '{print $1}' | sed -n "$((PHI_CARD + 1))p" || true)
fi
[ -n "$bdf" ] || { echo "phi-wait-vfio.sh: card $PHI_CARD is not enumerated (8086:225d)" >&2; exit 1; }

dev="/sys/bus/pci/devices/$bdf"
[ -d "$dev" ] || { echo "phi-wait-vfio.sh: $bdf is not a PCI device on this host" >&2; exit 1; }

# Three things have to have happened, and at host boot they happen in this
# order, asynchronously: systemd-modules-load inserts vfio_pci, it claims the
# card from the ids= option, and udev applies the group-phi ownership from
# the rule setup-arch.sh installs. Poll for the end state rather than trying
# to order against any of them.
for _ in $(seq 1 "$timeout"); do
    driver=$(basename "$(readlink -f "$dev/driver" 2>/dev/null || true)" 2>/dev/null || true)
    group=$(basename "$(readlink -f "$dev/iommu_group" 2>/dev/null || true)" 2>/dev/null || true)
    if [ "$driver" = vfio-pci ] && [ -n "$group" ] && [ "$group" != . ] && [ -w "/dev/vfio/$group" ]; then
        exit 0
    fi
    sleep 1
done

# Say which of the three is missing: each has a different fix.
driver=$(basename "$(readlink -f "$dev/driver" 2>/dev/null || true)" 2>/dev/null || true)
group=$(basename "$(readlink -f "$dev/iommu_group" 2>/dev/null || true)" 2>/dev/null || true)
if [ "$driver" != vfio-pci ]; then
    echo "phi-wait-vfio.sh: $bdf is bound to '${driver:-no driver}' after ${timeout}s, not vfio-pci." >&2
    echo "  sudo scripts/bind-vfio.sh binds it now; sudo scripts/setup-arch.sh makes it stick (scripts/setup-arch.md)." >&2
elif [ -z "$group" ] || [ "$group" = . ]; then
    echo "phi-wait-vfio.sh: $bdf has no IOMMU group after ${timeout}s; is the IOMMU enabled?" >&2
elif [ ! -e "/dev/vfio/$group" ]; then
    echo "phi-wait-vfio.sh: /dev/vfio/$group never appeared (${timeout}s)." >&2
else
    echo "phi-wait-vfio.sh: /dev/vfio/$group exists but is not writable by $(id -un) after ${timeout}s." >&2
    echo "  Membership of group phi is read at process start: log out and back in after sudo scripts/setup-arch.sh." >&2
    ls -l "/dev/vfio/$group" >&2 || true
fi
exit 1
