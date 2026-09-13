# phi-vfio / lib.rs

Crate root: the error type and `VfioPci`, which ties a container, a group and
a device together in the order the kernel requires:

1. open `/dev/vfio/vfio` (container);
2. open `/dev/vfio/<group>`, check it is viable;
3. `VFIO_GROUP_SET_CONTAINER`;
4. `VFIO_SET_IOMMU` on the container (only legal after at least one group is
   attached, which is why it is done here rather than in `Container::open`);
5. `VFIO_GROUP_GET_DEVICE_FD` with the BDF string.

## Why the legacy API

The container/group API is stable since Linux 3.6, documented in
`Documentation/driver-api/vfio.rst`, and needs no capability negotiation.
iommufd would add nothing this project uses. The kernel on this host has
both (`docs/hardware.md`).

## Permissions

`/dev/vfio/vfio` is world read/write. `/dev/vfio/<group>` is root-only until
the udev rule from `scripts/setup-arch.sh` chowns it to the `phi` group.
No capability is required beyond that; DMA mapping (`map_dma`) counts
against `RLIMIT_MEMLOCK`, raised by the same script.

## Testing

Unit tests in the submodules run without hardware. Opening a device is only
exercised by `phictl` and `phi-hw` when `PHI_BDF` is set.
