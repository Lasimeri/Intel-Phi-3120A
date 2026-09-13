# 0001: Host side is a Rust userspace program on VFIO

Status: accepted, 2026-09-13.

## Context

The host kernel (7.2.3) has no Xeon Phi driver; mainline removed
`drivers/misc/mic` in 5.10. The card's IOMMU group contains only the card.
VFIO, `vfio-pci`, iommufd, and the VFIO device cdev are all built as modules
on the CachyOS kernel. `CONFIG_RUST` is not enabled, so an in-kernel Rust
driver would require a custom kernel build.

Every host function Intel's driver performed is expressible from userspace:
BAR mmap for the aperture and MMIO, MSI-X via eventfds, DMA-mapped host
memory via the container's IOMMU mappings, config-space writes through the
VFIO config region.

## Decision

`phictl`/`phid` run as an unprivileged user in the `phi` group with the card
bound to `vfio-pci`. No host kernel module exists in this project.

## Consequences

- Safe Rust everywhere except the ioctl and MMIO layer in `phi-vfio`.
- No dependence on host kernel internals; survives host kernel upgrades.
- BAR0 mappings are uncached; bulk transfer must use the card DMA engine
  later. Acceptable: the boot image is 40 MB and the console is bytes.
- Requires `RLIMIT_MEMLOCK` raised for DMA pinning (setup script).

## Alternatives rejected

- Rust-for-Linux module: no `CONFIG_RUST`; unstable in-kernel API.
- Porting `mpss-modules` C: a treadmill the forks already demonstrate, and
  not Rust.
