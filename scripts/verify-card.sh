#!/usr/bin/env bash
# verify-card.sh: print the PCIe state of every Xeon Phi (or the one given)
# and exit non-zero if anything disqualifying is found. No root needed.
#   scripts/verify-card.sh [BDF]        # default: every 8086:225d device, or $PHI_BDF
# See verify-card.md.
set -uo pipefail

target="${1:-${PHI_BDF:-}}"
if [ -n "$target" ]; then
    bdfs=$target
else
    bdfs=$(lspci -Dn -d 8086:225d | awk '{print $1}' || true)
fi
if [ -z "$bdfs" ]; then
    echo "FAIL: no Xeon Phi 3120 series device (8086:225d) on the PCI bus"
    exit 1
fi
note() { printf '%-22s %s\n' "$1" "$2"; }

verify_one() {
    local bdf=$1 dev fail=0 parent bar0 bar4 b0s b0e b4s b4e drv grp members total f
    dev="/sys/bus/pci/devices/$bdf"
    [ -d "$dev" ] || { echo "FAIL: $bdf is not a PCI device on this host"; return 1; }

    note "device" "$bdf"
    note "ids" "$(cat "$dev/vendor"):$(cat "$dev/device") subsystem $(cat "$dev/subsystem_vendor"):$(cat "$dev/subsystem_device") rev $(cat "$dev/revision")"
    # Subsystem IDs seen on this project's cards. Both are 3120-series
    # boards (57 cores, 6 GB); 3608 was measured 2026-09-22 to expose an
    # 8 GiB BAR0 where 3c98 exposes 16 GiB, and boots the same image.
    case "$(cat "$dev/subsystem_device")" in
        0x3c98) note "sku" "3120A/3140A (linux-hardware.org decode of subsystem 3c98)";;
        0x3608) note "sku" "3120 series, subsystem 3608 (8 GiB BAR0 variant; measured 2026-09-22)";;
        *)      note "sku" "not 3c98 or 3608; a different 3120-series SKU";;
    esac

    # Link
    note "link" "$(cat "$dev/current_link_speed") x$(cat "$dev/current_link_width") (card max $(cat "$dev/max_link_speed") x$(cat "$dev/max_link_width"))"
    parent=$(readlink -f "$dev/..")
    note "parent bridge" "$(basename "$parent") max $(cat "$parent/max_link_speed" 2>/dev/null) x$(cat "$parent/max_link_width" 2>/dev/null)"

    # BARs: resource file lines are "start end flags"; BAR0 must be 64-bit, GiB class size.
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
    return $fail
}

fail=0
n=0
for b in $bdfs; do
    [ $n -gt 0 ] && echo
    verify_one "$b" || fail=1
    n=$((n + 1))
done
echo
note "cards" "$n"
# Kernel driver availability
if modinfo -n vfio-pci >/dev/null 2>&1; then note "vfio-pci module" "available"; else echo "FAIL: vfio-pci module missing"; fail=1; fi
exit $fail
