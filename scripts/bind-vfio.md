# bind-vfio.sh

Binds the card to `vfio-pci` using the kernel's `driver_override`
mechanism, or releases it. Root required (sysfs writes).

## Mechanism

1. Detect the BDF with `lspci -Dn -d 8086:225d` unless one is given or
   `PHI_BDF` is set.
2. Unbind from any current driver (there is none on a 5.10+ host, because
   mainline dropped `mic_host`).
3. Write `vfio-pci` to `driver_override`, then the BDF to
   `/sys/bus/pci/drivers_probe`. This is the same sequence `driverctl` and
   libvirt use.
4. Print the IOMMU group and the `/dev/vfio/<group>` node, which the udev
   rule from `setup-arch.sh` chowns to the `phi` group.

`unbind` reverses it and clears `driver_override`, leaving the card with no
driver, exactly the state a fresh boot leaves it in.

## Its relationship to the persistent binding

Since 2026-09-14 `setup-arch.sh` writes `/etc/modprobe.d/phi-vfio.conf`
(`options vfio-pci ids=8086:225d`), so `vfio-pci` claims the card when it
loads at boot and this script is not part of the routine. It stays for
three cases:

- A host set up before that change, or one where the file was removed:
  `vfio-pci` loads with no ids and the card comes up with no driver
  (`scripts/setup-arch.md` has the symptom and the checks).
- Handing the card back: `unbind` clears `driver_override` and leaves it
  with no driver, the state a fresh boot used to leave it in. That is what
  the QEMU passthrough path (the oracle VM, never needed in the end) would
  want.
- Binding without a reboot after changing either file.

## Measured on this host

Group 30, `0000:2e:00.0`, sole member. Reserved IOVA ranges in the group are
listed in `docs/hardware.md`.
