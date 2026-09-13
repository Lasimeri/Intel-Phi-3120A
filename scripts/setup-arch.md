# setup-arch.sh

Prepares an Arch Linux host. Root required. Idempotent.

## What it changes and why

| Step | Change | Reason |
| --- | --- | --- |
| Packages | Required: `base-devel git curl pciutils usbutils clang lld llvm tcc cpio bc flex bison libelf openssl perl`, plus `rust` (default) or `rustup` (`PHI_RUSTUP=1`). Optional, installed one by one and only warned about on failure: `poppler musl kernel-headers-musl` | Host workspace build, kernel build prerequisites; the optional ones are for the card toolchain (P2) and PDF text extraction |
| Group `phi` | Created as a system group; the invoking `sudo` user is added | Unprivileged access to the VFIO group device |
| `/etc/udev/rules.d/80-phi-vfio.rules` | `/dev/vfio/<N>` owned by `phi`, mode 0660 | Same |
| `/etc/security/limits.d/80-phi.conf` | Unlimited memlock for `@phi` | VFIO DMA mappings pin memory (phase P6) |
| `/etc/modules-load.d/phi-vfio.conf` | `vfio-pci`, `vfio_iommu_type1` at boot | The card is bound to `vfio-pci` by `bind-vfio.sh` |

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
