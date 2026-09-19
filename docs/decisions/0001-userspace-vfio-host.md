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

`phictl` runs as an unprivileged user in the `phi` group with the card
bound to `vfio-pci`. No host kernel module exists in this project.

## Consequences

- Safe Rust everywhere except the ioctl and MMIO layer in `phi-vfio`.
- No dependence on host kernel internals; survives host kernel upgrades.
- BAR0 mappings are uncached; bulk transfer must use the card DMA engine
  later. Acceptable: the boot image is 8.4 MB and the console is bytes.
- Requires `RLIMIT_MEMLOCK` raised for DMA pinning (setup script).

## What it became (2026-09-19)

- There is no separate `phid`. The daemon is the `phictl boot --serve`
  process: it holds the VFIO device for the card's lifetime and serves
  everything else over a Unix socket (ADR 0008, ADR 0009).
- The DMA engine arrived on 2026-09-16 (`host/crates/phi-hw/src/dma.rs`,
  `docs/results/2026-09-16-dma.md`): 3.58 GB/s memory to memory against
  the aperture's tens of MB/s. The uncached BAR0 path remains for the
  boot image, the rings and `--no-dma`.
- The claim about surviving host kernel upgrades has held across 7.2.3 to
  7.2.6 with no change to this project. What does not survive a reboot on
  its own is the `vfio-pci` binding unless `/etc/modprobe.d/phi-vfio.conf`
  is present (`scripts/setup-arch.md`).

## Alternatives rejected

- Rust-for-Linux module: no `CONFIG_RUST`; unstable in-kernel API.
- Porting `mpss-modules` C: a treadmill the forks already demonstrate, and
  not Rust.
