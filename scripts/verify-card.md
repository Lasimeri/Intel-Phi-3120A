# verify-card.sh

Read-only health check of the card's PCIe state, usable without root. It
reads sysfs only, so it works whether or not a driver is bound.

## Checks and what a failure means

| Check | Failure meaning |
| --- | --- |
| Device `8086:225d` present | Card not seated, not powered (both auxiliary connectors are required), or slot disabled |
| BAR0 assigned | Firmware could not place a 16 GiB BAR: enable Above 4G Decoding |
| BAR4 assigned | Same class of problem, or a bridge window shortage |
| IOMMU group exists | IOMMU disabled in firmware or kernel |
| `vfio-pci` module available | Kernel built without VFIO |

Warnings (non-fatal): link narrower than x16 (expected x8 on this host's
bifurcated slot), other devices in the IOMMU group (they would all need to be
bound to VFIO).

## Output on this host (2026-09-13)

```
device                 0000:2e:00.0
ids                    0x8086:0x225d subsystem 0x8086:0x3c98 rev 0x20
sku                    3120A/3140A
link                   5.0 GT/s PCIe x8 (card max 5.0 GT/s PCIe x16)
parent bridge          0000:00:03.1 max 16.0 GT/s PCIe x8
BAR0 aperture          0x0000007c00000000 .. 0x0000007fffffffff (16 GiB)
BAR4 mmio              0x00000000fce00000 .. 0x00000000fce1ffff (128 KiB)
driver                 none
iommu group            30: 0000:2e:00.0
```
