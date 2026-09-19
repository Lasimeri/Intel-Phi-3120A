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
| `/etc/modprobe.d/phi-vfio.conf` | `options vfio-pci ids=8086:225d` | Added 2026-09-14. vfio-pci claims the card the moment it loads, so a rebooted host needs no binding step and `bind-vfio.sh` becomes a repair tool rather than part of the routine. Read only at module load; it leaves no sysfs file (see below) |

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
ls -l /etc/modprobe.d/phi-vfio.conf                 # must exist
modprobe --showconfig | grep vfio_pci               # must show: options vfio_pci ids=8086:225d
readlink /sys/bus/pci/devices/0000:2e:00.0/driver   # must end in vfio-pci
```

The `ids` parameter is read only when `vfio-pci` loads. If
`/etc/modprobe.d/phi-vfio.conf` is missing, `modules-load.d` still loads
the module at boot but with no ids, so it claims nothing and the card is
left with no driver.

Do not look for `/sys/module/vfio_pci/parameters/ids`. That file does not
exist whether the option is set or not: `vfio-pci` declares the parameter
with permission 0 (`module_param_string(ids, ids, sizeof(ids), 0)`), so no
sysfs entry is created. `modinfo vfio-pci` lists the parameter,
`modprobe --showconfig` shows whether the option is registered (as
`vfio_pci`, with the dash normalised to an underscore), and the binding
itself is the only evidence that it took effect. Checked on this host,
2026-09-19, kernel 7.2.6-1-cachyos.

That is what happened on the development host on 2026-09-19. It had been
set up on 2026-09-13, before the change that writes the `modprobe.d` file,
so after the reboot to kernel 7.2.6-1-cachyos `vfio-pci` was loaded by
`modules-load.d` with no ids, claimed nothing, and `phi.service` restarted
every 10 s. `sudo scripts/setup-arch.sh` wrote the file (`modprobe
--showconfig` now shows the option) and `sudo scripts/bind-vfio.sh` bound
the card in place, after which the unit came up and the card booted with
its disk and host memory.

That binding came from `driver_override`, not from the module option: the
module was already loaded when the file was written, and the option is read
only at load time. So it proved nothing about the next boot.

## Reboot survival, observed 2026-09-19

The host rebooted at 03:23:18 with the `modprobe.d` file in place, and the
card came up bound with nothing run by hand:

```
$ readlink /sys/bus/pci/devices/0000:2e:00.0/driver
../../../../bus/pci/drivers/vfio-pci
$ cat /sys/bus/pci/devices/0000:2e:00.0/driver_override
(null)
```

`driver_override` reading `(null)` is what makes this conclusive:
`bind-vfio.sh` sets it, and sysfs does not carry it across a reboot, so the
binding cannot have come from there. The kernel log shows where it did come
from:

```
03:23:27.307  systemd-modules-load[399]: Inserted module 'vfio_pci'
03:23:27.338  kernel: vfio_pci: add [8086:225d[ffffffff:ffffffff]] class 0x000000/00000000
03:27:25.898  kernel: vfio-pci 0000:2e:00.0: enabling device (0000 -> 0002)
```

`modules-load.d` inserts the module nine seconds into the boot, the `ids`
option makes it register the card's id at that moment, and `phi.service`
opens the device four minutes later when the user's session starts. The
whole path from cold host to a running card is unattended.

## What the card can do without root

With the `phi` group and the card bound, `scripts/phi-up.sh` boots and
serves it from an unprivileged shell. Only the optional network bridge
(`--net`, which creates a TAP device) needs root; the port forwarder
(`--forward`) does not.
