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

## Why no persistent binding

A persistent `vfio-pci.ids=8086:225d` on the kernel command line would also
work. It is not done by default so that a host reboot always starts from the
known state (no driver, card in bootstrap), and so the oracle VM path (QEMU
passthrough) stays available without fighting a binding.

## Measured on this host

Group 30, `0000:2e:00.0`, sole member. Reserved IOVA ranges in the group are
listed in `docs/hardware.md`.
