#!/usr/bin/env bash
# setup-arch.sh: prepare an Arch Linux host for the Intel Phi 3120A project.
# Idempotent. Run as root (sudo). See setup-arch.md for what and why.
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "setup-arch.sh: run as root (sudo scripts/setup-arch.sh)" >&2
    exit 1
fi

# --- 1. Packages -------------------------------------------------------------
# Kept minimal on purpose. Card-side toolchain builds (LLVM, musl, kernel)
# document their own additional packages under toolchain/ and card/.
# Required for the host workspace and the scripts.
packages=(
    base-devel git curl
    pciutils usbutils
    clang lld llvm
    tcc
    cpio bc flex bison libelf openssl perl
    cmake ninja        # LLVM and kernel-adjacent builds (toolchain/)
    jq                 # target-spec generation (toolchain/rust/gen-target.sh)
)
# Optional: only needed for the card toolchain (phase P2) and for reading
# the Intel PDFs. Installed one by one so a missing mirror file (seen on
# CachyOS v3 mirrors for musl on 2026-09-13) does not abort the setup.
optional=(
    poppler            # pdftotext, used to read Intel PDFs from vendor/
    musl kernel-headers-musl
)
# Rust: the distro `rust` package is enough for the host workspace. rustup is
# needed later for the card target (build-std on nightly); it conflicts with
# `rust`, so it is opt-in: PHI_RUSTUP=1 sudo scripts/setup-arch.sh
if [ "${PHI_RUSTUP:-0}" = "1" ]; then
    packages+=(rustup)
else
    packages+=(rust)
fi
pacman -S --needed --noconfirm "${packages[@]}"
for p in "${optional[@]}"; do
    pacman -S --needed --noconfirm "$p" || echo "WARNING: optional package $p failed to install; needed only for the card toolchain" >&2
done

# --- 2. Group and udev --------------------------------------------------------
getent group phi >/dev/null || groupadd --system phi
cat > /etc/udev/rules.d/80-phi-vfio.rules <<'RULE'
# Intel Phi 3120A project: give the phi group access to VFIO group devices.
SUBSYSTEM=="vfio", KERNEL=="[0-9]*", GROUP="phi", MODE="0660"
RULE
udevadm control --reload
udevadm trigger --subsystem-match=vfio || true

# --- 3. Memlock limit ---------------------------------------------------------
# VFIO pins every page it maps for DMA; the default 8 MiB memlock is far too
# small once the DMA arena exists (phase P6).
cat > /etc/security/limits.d/80-phi.conf <<'LIM'
@phi soft memlock unlimited
@phi hard memlock unlimited
LIM

# --- 4. Modules at boot -------------------------------------------------------
cat > /etc/modules-load.d/phi-vfio.conf <<'MOD'
vfio-pci
vfio_iommu_type1
MOD
modprobe vfio-pci
modprobe vfio_iommu_type1

# --- 5. Add the invoking user to the group ------------------------------------
if [ -n "${SUDO_USER:-}" ] && ! id -nG "$SUDO_USER" | tr ' ' '\n' | grep -qx phi; then
    usermod -aG phi "$SUDO_USER"
    echo "Added $SUDO_USER to group phi. Log out and back in for it to apply."
fi

# --- 6. Sanity checks (informational) -----------------------------------------
if [ -z "$(ls -A /sys/kernel/iommu_groups 2>/dev/null)" ]; then
    echo "WARNING: no IOMMU groups. Enable AMD-Vi / VT-d in firmware." >&2
fi
bdf=$(lspci -Dn -d 8086:225d | awk '{print $1}' | head -1 || true)
if [ -z "$bdf" ]; then
    echo "WARNING: no Xeon Phi 3120 series device (8086:225d) enumerated." >&2
else
    echo "Xeon Phi found at $bdf"
    if ! grep -q "0x0000007" "/sys/bus/pci/devices/$bdf/resource"; then
        echo "WARNING: BAR0 does not look like a 64-bit assignment; check Above-4G decoding." >&2
    fi
fi
echo "setup-arch.sh: done"
