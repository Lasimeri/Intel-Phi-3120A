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
and the kernel pins it. The caller allocates page-aligned, `mlock`-able
memory (`mmap` anonymous, or a shared file for `--host-mem`) and keeps it
alive for the mapping's lifetime. Page sizes the IOMMU can use are
reported by `iova_page_sizes`; on this host's AMD-Vi that includes 4 KiB,
2 MiB and 1 GiB.

The callers today are `phi-hw`'s DMA engine (descriptor ring and staging
buffers) and `phictl`'s host-memory service, which maps one shared file so
the card's `/dev/phiblk1` and `/dev/phihost` reach it.

Reserved IOVA ranges (`docs/hardware.md`) are not enforced here; the kernel
rejects a mapping that collides with a reserved region with `EINVAL`, which
is what the callers rely on.
