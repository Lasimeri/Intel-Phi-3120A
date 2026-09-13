# phi-vfio / group.rs

One IOMMU group. On this host the card is alone in group 30
(`docs/hardware.md`), which is the ideal case: no other device has to be
detached from its driver to make the group viable.

## Viability

`VFIO_GROUP_GET_STATUS` reports `VIABLE` only when every device in the
group is either bound to `vfio-pci` or has no driver. If the check fails the
error names the group so the user can list `/sys/kernel/iommu_groups/<N>/devices`.

## Device name

`VFIO_GROUP_GET_DEVICE_FD` takes the device's sysfs name, which for PCI is
the full BDF with domain (`0000:2e:00.0`). Passing `2e:00.0` fails with
`ENODEV`.
