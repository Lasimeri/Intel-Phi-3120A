# verify-card.sh

Prints the PCIe state of every Xeon Phi on the bus (or the one given) and
exits non-zero if anything disqualifying is found. No root needed.

```
scripts/verify-card.sh          # every 8086:225d device
scripts/verify-card.sh BDF      # one
```

Per card: vendor, device, subsystem and revision, the SKU decode, the
negotiated link against the card's and the parent bridge's maximum, BAR0
(the aperture, must be assigned and GiB-sized: "UNASSIGNED" means Above
4G Decoding is off in firmware) and BAR4 (128 KiB MMIO), the driver, the
IOMMU group and its members (a shared group means VFIO needs all of
them), the reserved IOVA ranges, and the AER counters.

Two subsystem IDs are known on this project's cards, both 3120 series,
57 cores, 6 GB, booting the same image:

| subsystem | BAR0 | seen |
| --- | --- | --- |
| `3c98` | 16 GiB | the original 3120A, 2026-09-13 |
| `3608` | 8 GiB | the second card, 2026-09-22 (chipset slot, Gen2 x4) |

The link width is whatever the slot gives: x8 on the CPU slot that is
bifurcated with the GPU, x4 on the chipset slot. Both are below the
card's x16 and neither is a fault.
