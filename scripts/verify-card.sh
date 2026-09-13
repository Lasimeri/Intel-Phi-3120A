#!/usr/bin/env bash
# verify-card.sh: print the PCIe state of the Xeon Phi and exit non-zero if
# anything disqualifying is found. No root needed. See verify-card.md.
set -uo pipefail

bdf="${1:-${PHI_BDF:-}}"
if [ -z "$bdf" ]; then
    bdf=$(lspci -Dn -d 8086:225d | awk '{print $1}' | head -1 || true)
fi
if [ -z "$bdf" ]; then
    echo "FAIL: no Xeon Phi 3120 series device (8086:225d) on the PCI bus"
    exit 1
fi
dev="/sys/bus/pci/devices/$bdf"
fail=0
note() { printf '%-22s %s\n' "$1" "$2"; }

note "device" "$bdf"
note "ids" "$(cat "$dev/vendor"):$(cat "$dev/device") subsystem $(cat "$dev/subsystem_vendor"):$(cat "$dev/subsystem_device") rev $(cat "$dev/revision")"
case "$(cat "$dev/subsystem_device")" in
    0x3c98) note "sku" "3120A/3140A (linux-hardware.org decode of subsystem 3c98)";;
    *)      note "sku" "not 3c98; a different 3120-series SKU";;
esac

# Link
note "link" "$(cat "$dev/current_link_speed") x$(cat "$dev/current_link_width") (card max $(cat "$dev/max_link_speed") x$(cat "$dev/max_link_width"))"
parent=$(readlink -f "$dev/..")
note "parent bridge" "$(basename "$parent") max $(cat "$parent/max_link_speed" 2>/dev/null) x$(cat "$parent/max_link_width" 2>/dev/null)"

# BARs: resource file lines are "start end flags"; BAR0 must be 64-bit, 16 GiB class size.
bar0=$(sed -n '1p' "$dev/resource")
bar4=$(sed -n '5p' "$dev/resource")
set -- $bar0; b0s=$1; b0e=$2
set -- $bar4; b4s=$1; b4e=$2
if [ "$b0s" = "0x0000000000000000" ]; then
    note "BAR0" "UNASSIGNED"; echo "FAIL: BAR0 not assigned. Enable Above 4G Decoding in firmware."; fail=1
else
    note "BAR0 aperture" "$b0s .. $b0e ($(( ( $(printf '%d' "$b0e") - $(printf '%d' "$b0s") + 1 ) >> 30 )) GiB)"
fi
if [ "$b4s" = "0x0000000000000000" ]; then
    note "BAR4" "UNASSIGNED"; echo "FAIL: BAR4 (MMIO) not assigned."; fail=1
else
    note "BAR4 mmio" "$b4s .. $b4e ($(( ( $(printf '%d' "$b4e") - $(printf '%d' "$b4s") + 1 ) >> 10 )) KiB)"
fi

# Driver and IOMMU
if [ -L "$dev/driver" ]; then drv=$(basename "$(readlink -f "$dev/driver")"); else drv=none; fi
note "driver" "$drv"
if [ -L "$dev/iommu_group" ]; then
    grp=$(basename "$(readlink -f "$dev/iommu_group")")
    members=$(ls "/sys/kernel/iommu_groups/$grp/devices" | tr '\n' ' ')
    note "iommu group" "$grp: $members"
    if [ "$(ls "/sys/kernel/iommu_groups/$grp/devices" | wc -l)" -ne 1 ]; then
        echo "WARN: group has other members; VFIO will require all of them to be bound."
    fi
    note "reserved iova" "$(tr '\n' ';' < "/sys/kernel/iommu_groups/$grp/reserved_regions")"
    if [ -e "/dev/vfio/$grp" ]; then note "vfio node" "$(ls -l "/dev/vfio/$grp")"; fi
else
    note "iommu group" "NONE"; echo "FAIL: no IOMMU group. Enable the IOMMU in firmware (and intel_iommu=on on Intel hosts)."; fail=1
fi

# AER
for f in aer_dev_correctable aer_dev_nonfatal aer_dev_fatal; do
    if [ -r "$dev/$f" ]; then
        total=$(grep TOTAL "$dev/$f" | awk '{print $2}')
        note "$f" "$total"
    fi
done

# Kernel driver availability
if modinfo -n vfio-pci >/dev/null 2>&1; then note "vfio-pci module" "available"; else echo "FAIL: vfio-pci module missing"; fail=1; fi

exit $fail
