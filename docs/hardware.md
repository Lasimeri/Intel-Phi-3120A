# Hardware

## The card

Intel Xeon Phi coprocessor **3120A** (Knights Corner, x100 family).
Identification on this machine, measured 2026-09-13 with `lspci -nn`:

```
2e:00.0 Co-processor [0b40]: Intel Corporation Xeon Phi coprocessor 3120 series [8086:225d] (rev 20)
        Subsystem: Intel Corporation Device 3c98
```

Subsystem `8086:3c98` decodes to "3120A/3140A" in the linux-hardware.org
database; the 3120P uses a different subsystem ID. The A suffix means the
active-cooling SKU with an on-card blower.

| Property | Value | Source |
| --- | --- | --- |
| Cores / hardware threads | 57 cores, 4 threads per core, 228 threads | Intel ARK SKU 75797 |
| Core clock | 1.10 GHz | Intel ARK |
| L2 | 512 KiB per core, 28.5 MiB total, coherent ring | Intel ARK, SSDG 2.1 |
| Vector unit | 512-bit VPU per core, 32 zmm registers, 8 mask registers, 16 SP / 8 DP lanes, FMA | ISA reference 327364 |
| Memory | 6 GB GDDR5, 12 channels active on this SKU, 240 GB/s, ECC capable | Intel ARK |
| PCIe | Gen2 x16 capable | Datasheet 328209 |
| TDP | 300 W, active cooling, PCIe 6-pin plus 8-pin auxiliary power | Datasheet 328209 table 2-1 |
| CPU family/model | family 0x0B, model 0x01 | ISA reference App. B, CPUID leaf 1 |
| ISA | x86-64 base minus a long list of instructions, plus the KNC vector ISA | see `research/isa-deletions.md` |

## The host

| Item | Value |
| --- | --- |
| CPU | AMD Ryzen 7 5800X |
| Chipset | AMD X570 (Matisse), CPU-direct PCIe x16 bifurcated x8/x8 |
| RAM | 64 GiB |
| Host kernel | 7.2.3-1-cachyos (measured), Arch-based |
| IOMMU | AMD-Vi enabled, interrupt remapping enabled, DMA domain lazy TLB invalidation |
| Other GPU | NVIDIA RTX 3090 Ti on the sibling x8 bridge (00:03.2, bus 2f) |

## Measured PCIe state (2026-09-13)

| Item | Value | Note |
| --- | --- | --- |
| Link | Gen2 (5 GT/s) x8 | Card caps Gen2 x16. Parent bridge 00:03.1 caps x8. Slot-limited; no training fault. |
| BAR0 (MEMBAR0, aperture) | 0x7c00000000, 16 GiB, 64-bit prefetchable | Above-4G decoding works. Card GDDR is 6 GB; the aperture is oversized by design. |
| BAR4 (MEMBAR1, MMIO) | 0xfce00000, 128 KiB, 64-bit | DBOX registers in the first 64 KiB, SBOX in the second (SSDG 2.1.12). |
| Command register | 0x0000 | Memory decode and bus master off until a driver enables them. |
| AER counters | all zero | |
| IOMMU group | 30, contains only the card | VFIO passthrough needs no ACS override. |
| Reserved IOVA in group 30 | 0xfee00000-0xfeefffff (MSI), 0xfd00000000-0xffffffffff | The DMA arena allocator must avoid both. |
| Interrupt pin | B, unrouted | Expected without a driver. |

Commands used: `lspci -nn`, `lspci -vvv -s 2e:00.0`, `cat /sys/bus/pci/devices/0000:2e:00.0/{current_link_speed,current_link_width,resource}`, `cat /sys/kernel/iommu_groups/30/reserved_regions`, `setpci -s 2e:00.0 COMMAND`.

## Power

300 W for the card plus a 3090 Ti at stock limits exceeds the 900 W UPS on this
host. Rule: power-limit the GPU (`nvidia-smi -pl`) or keep the card idle during
GPU work. Card idle power with the cores clock-gated is low (SSDG 2.1.13), but
a booted kernel that spins 228 threads is not idle.

## Why x8 is the ceiling

The Matisse x16 slot is split x8/x8 across bridges 00:03.1 (Phi, bus 2e) and
00:03.2 (3090 Ti, bus 2f). Moving the Phi to full x16 would push the GPU onto
the chipset's Gen4 x4 link. Gen2 x8 gives about 4 GB/s per direction raw,
roughly 3.2 GB/s usable, which is far more than SSH and a console need.
