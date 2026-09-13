# phi-vfio / sysfs.rs

Read-only sysfs helpers. They exist so that `phictl` can work with no
arguments on a machine with one card, and so that opening the wrong device
through VFIO (a GPU, say) is refused before any ioctl.

- `iommu_group_of`: follows the `iommu_group` symlink. A missing link is
  the "IOMMU disabled in firmware" symptom, and the error says so.
- `find_phi`: scans `/sys/bus/pci/devices` for vendor 0x8086 device 0x225d
  and returns the lowest BDF. Multiple cards are possible in principle;
  this project has one, and `--bdf` selects explicitly.
- `resolve_bdf`: precedence is command-line, then `PHI_BDF`, then scan.

Paths are the standard sysfs layout documented in
`Documentation/ABI/testing/sysfs-bus-pci`.
