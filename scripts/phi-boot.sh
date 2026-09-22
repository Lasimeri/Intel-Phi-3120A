#!/usr/bin/env bash
# phi-boot.sh: boot one card in the foreground with everything a card
# gets on this host: its control socket, its SSH forward, its disk image,
# its host memory, its own ring subnet and hostname. The systemd template
# unit phi@.service runs this with the instance number; phi-up.sh runs it
# in the background. Both paths therefore boot a card identically.
#
#   scripts/phi-boot.sh [-c N] [--toolchain-later] [extra phictl boot args]
#
# Console output goes to stdout (the journal under systemd, console.log
# under phi-up.sh). See phi-boot.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
. "$root/scripts/phi-env.sh"
phi_env "$@"
set -- "${PHI_ARGS[@]}"

P="$root/host/target/debug/phictl"
kernel="$root/card/kernel/build/out/arch/x86/boot/bzImage"
initrd="$root/card/initramfs/build/initramfs.cpio.gz"
[ -x "$P" ] || { echo "phi-boot.sh: build the host tools first (make build)" >&2; exit 1; }
[ -s "$kernel" ] && [ -s "$initrd" ] || { echo "phi-boot.sh: kernel or initramfs missing" >&2; exit 1; }
[ "$PHI_PRESENT" = yes ] || { echo "phi-boot.sh: card $PHI_CARD is not on the PCI bus (phictl cards)" >&2; exit 1; }

args=(--card "$PHI_CARD" boot --kernel "$kernel" --initrd "$initrd"
      --cmdline "earlyprintk=phiring console=ttyPHI0 PHI_CARD_ADDR=$PHI_CARD_IP/24 PHI_HOSTNAME=$PHI_HOSTNAME${PHI_CMDLINE_EXTRA:+ $PHI_CMDLINE_EXTRA}"
      --serve "$PHI_SOCK" --forward "$PHI_PORT:22" --net-addr "$PHI_HOST_IP/24")
if [ -n "$PHI_DISK_SEL" ]; then
    [ -f "$PHI_DISK_SEL" ] || { echo "phi-boot.sh: disk image $PHI_DISK_SEL not found (scripts/phi-disk.sh create $PHI_DISK_SEL SIZE)" >&2; exit 1; }
    args+=(--disk "$PHI_DISK_SEL")
fi
[ -n "$PHI_HOSTMEM" ] && [ "$PHI_HOSTMEM" != none ] && args+=(--host-mem "$PHI_HOSTMEM")

mkdir -p "$PHI_RUNDIR"; chmod 700 "$PHI_RUNDIR"
exec env PHICTL_SOCKET="$PHI_SOCK" "$P" "${args[@]}" "$@"
