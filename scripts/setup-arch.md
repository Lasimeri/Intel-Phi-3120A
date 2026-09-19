# setup-arch.sh

Prepares an Arch Linux host. Root required. Idempotent.

## What it changes and why

| Step | Change | Reason |
| --- | --- | --- |
| Packages | Required: `base-devel git curl pciutils usbutils clang lld llvm tcc cpio bc flex bison libelf openssl perl cmake ninja jq`, plus `rust` (default) or `rustup` (`PHI_RUSTUP=1`). Optional, installed one by one and only warned about on failure: `poppler musl kernel-headers-musl` | Host workspace build, kernel build prerequisites; the optional ones are for the card toolchain (P2) and PDF text extraction |
| Group `phi` | Created as a system group; the invoking `sudo` user is added | Unprivileged access to the VFIO group device |
| `/etc/udev/rules.d/80-phi-vfio.rules` | `/dev/vfio/<N>` owned by `phi`, mode 0660 | Same |
| `/etc/security/limits.d/80-phi.conf` | Unlimited memlock for `@phi` | VFIO DMA mappings pin memory (phase P6) |
| `/etc/modules-load.d/phi-vfio.conf` | `vfio-pci`, `vfio_iommu_type1` at boot | Both modules present before anything wants them |
| `/etc/modprobe.d/phi-vfio.conf` | `options vfio-pci ids=8086:225d` | Added 2026-09-14. vfio-pci claims the card the moment it loads, so a rebooted host needs no binding step and `bind-vfio.sh` becomes a repair tool rather than part of the routine |

## What it deliberately does not do

- It does not bind the card (`bind-vfio.sh` does, explicitly).
- It does not change the kernel command line. On AMD hosts the IOMMU is on
  by default when enabled in firmware; on Intel hosts add `intel_iommu=on`
  yourself if `/sys/kernel/iommu_groups` is empty.
- It does not install the card-side toolchain; see `toolchain/README.md`.

## Verification

```sh
ls -l /dev/vfio/            # after bind-vfio.sh: a numbered node owned by phi
ulimit -l                   # unlimited in a fresh login shell
id                          # contains phi
```

## If the card is unbound after a reboot

Symptom: `phictl` exits with
`0000:2e:00.0 is not bound to vfio-pci; run: sudo scripts/bind-vfio.sh`,
and the autoboot unit sits in `activating (auto-restart)` forever.

Check, in this order:

```sh
ls -l /etc/modprobe.d/phi-vfio.conf     # must exist
cat /sys/module/vfio_pci/parameters/ids # must contain 8086:225d
readlink /sys/bus/pci/devices/0000:2e:00.0/driver   # must end in vfio-pci
```

The `ids` parameter is read only when `vfio-pci` loads. If
`/etc/modprobe.d/phi-vfio.conf` is missing, `modules-load.d` still loads
the module at boot but with no ids, so it claims nothing and the card is
left with no driver.

That is the state of the host this was written on (2026-09-19, kernel
7.2.6-1-cachyos): `/etc/modules-load.d/phi-vfio.conf` is dated 2026-09-13
and `/etc/modprobe.d/phi-vfio.conf` does not exist, because this host was
set up before the 2026-09-14 change that writes it. Re-running
`sudo scripts/setup-arch.sh` writes the file; `sudo scripts/bind-vfio.sh`
binds the card without a reboot.

Reboot survival with the file in place has not been observed on this host
yet. Verify it the first time by rebooting and checking `readlink` above
before assuming the autoboot unit will come up on its own.

## What the card can do without root

With the `phi` group and the card bound, `scripts/phi-up.sh` boots and
serves it from an unprivileged shell. Only the optional network bridge
(`--net`, which creates a TAP device) needs root; the port forwarder
(`--forward`) does not.
