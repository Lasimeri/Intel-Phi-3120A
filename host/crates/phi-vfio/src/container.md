# phi-vfio / container.rs

The container is the IOMMU domain. Every DMA mapping made through it is
visible to every device in every group attached to it; this project attaches
exactly one group.

## Ordering constraint

`VFIO_SET_IOMMU` fails with `EINVAL` until at least one group is attached.
`VfioPci::open` therefore does: open container, open group, set container on
group, *then* set IOMMU. This is documented in
`Documentation/driver-api/vfio.rst` and easy to get wrong.

## DMA mapping

`map_dma` is `unsafe` because the device gains write access to the memory
and the kernel pins it. The caller (phase P6's DMA arena) allocates
page-aligned, `mlock`-able memory (`mmap` anonymous, or hugetlbfs) and keeps
it alive for the mapping's lifetime. Page sizes the IOMMU can use are
reported by `iova_page_sizes`; on this host's AMD-Vi that includes 4 KiB,
2 MiB and 1 GiB.

Reserved IOVA ranges (`docs/hardware.md`) are not enforced here; the arena
allocator in phase P6 enforces them, and the kernel rejects a mapping that
collides with a reserved region with `EINVAL`.
