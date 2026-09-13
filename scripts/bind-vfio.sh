#!/usr/bin/env bash
# bind-vfio.sh: hand the Xeon Phi to vfio-pci, or take it back.
# Usage: sudo scripts/bind-vfio.sh [bind|unbind] [BDF]
# See bind-vfio.md.
set -euo pipefail

action="${1:-bind}"
case "$action" in bind|unbind) ;; *) echo "usage: bind-vfio.sh [bind|unbind] [BDF]" >&2; exit 2;; esac
bdf="${2:-${PHI_BDF:-}}"
if [ -z "$bdf" ]; then
    bdf=$(lspci -Dn -d 8086:225d | awk '{print $1}' | head -1 || true)
fi
if [ -z "$bdf" ]; then
    echo "bind-vfio.sh: no 8086:225d device found and no BDF given" >&2
    exit 1
fi
dev="/sys/bus/pci/devices/$bdf"
if [ ! -d "$dev" ]; then
    echo "bind-vfio.sh: $dev does not exist" >&2
    exit 1
fi
if [ "$(id -u)" -ne 0 ]; then
    echo "bind-vfio.sh: run as root" >&2
    exit 1
fi

if [ -L "$dev/driver" ]; then current=$(basename "$(readlink -f "$dev/driver")"); else current=none; fi

case "$action" in
bind)
    modprobe vfio-pci
    if [ "$current" = "vfio-pci" ]; then
        echo "$bdf already bound to vfio-pci"
    else
        if [ "$current" != "none" ] && [ -e "$dev/driver/unbind" ]; then
            echo "$bdf" > "$dev/driver/unbind"
        fi
        echo vfio-pci > "$dev/driver_override"
        echo "$bdf" > /sys/bus/pci/drivers_probe
        sleep 0.2
        if [ -L "$dev/driver" ]; then now=$(basename "$(readlink -f "$dev/driver")"); else now=none; fi
        if [ "$now" != "vfio-pci" ]; then
            echo "bind-vfio.sh: bind failed, driver is '$now'" >&2
            exit 1
        fi
        echo "$bdf bound to vfio-pci"
    fi
    group=$(basename "$(readlink -f "$dev/iommu_group")")
    echo "IOMMU group $group -> /dev/vfio/$group"
    ls -l "/dev/vfio/$group"
    ;;
unbind)
    if [ "$current" = "vfio-pci" ]; then
        echo "$bdf" > "$dev/driver/unbind"
    fi
    echo > "$dev/driver_override"
    echo "$bdf released (no driver)"
    ;;
*)
    echo "usage: bind-vfio.sh [bind|unbind] [BDF]" >&2
    exit 2
    ;;
esac
