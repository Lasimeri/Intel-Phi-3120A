# vfio.S: one device through VFIO

The legacy container and group API (`/dev/vfio/vfio`, `/dev/vfio/<group>`)
with the type1v2 IOMMU backend, as `host/crates/phi-vfio` used it.

`vfio_open(bdf)`, in order: the device's IOMMU group from sysfs; the
container opened and checked (`VFIO_GET_API_VERSION` 0,
`VFIO_CHECK_EXTENSION` type1v2); the group opened and checked viable
(`VFIO_GROUP_GET_STATUS`); the group attached
(`VFIO_GROUP_SET_CONTAINER`); the backend chosen (`VFIO_SET_IOMMU`, legal
only after a group is attached); the device fd
(`VFIO_GROUP_GET_DEVICE_FD`); `VFIO_DEVICE_GET_INFO`; the configuration
region's offset and size (`VFIO_DEVICE_GET_REGION_INFO` index 7);
Memory Space Enable and Bus Master Enable set in COMMAND (a no-op:
`vfio-pci` enables the device on open, COMMAND read 0x0006 at first
contact); BAR 4 (registers) and BAR 0 (aperture, 8 or 16 GiB) mapped
whole, read/write, shared. Every failure exits naming the call.

Opening the device resets the card (`vfio-pci` issues a function reset
on open and on close), so a `phictl` command that opens the card ends
the card's session and the bootstrap needs about ten seconds to retrain
GDDR afterwards; a second opener of the same group is refused by the
kernel while a daemon holds it.

`cfg_read16` and `cfg_write16` use `pread`/`pwrite` at the config
region's offset. `vfio_map_dma` pins a page-aligned range of this
process's memory and maps it at an IOVA for the device, readable and
writable (`VFIO_IOMMU_MAP_DMA`); the pages stay pinned until the
container closes with the process.

Structure layouts: `defs.md`.
