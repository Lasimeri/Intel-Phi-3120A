# Memory map: how card and host see each other

Sources: SSDG 328207-002 section 2.1.12; `arch/x86/kernel/intelmic.c` in the
Intel k1om tree (SMPT constants); mainline v5.9 `drivers/misc/mic/host/mic_x100.c`
(`mic_x100_smpt_hw_init`); measurements on this host.

## Card physical address space (40-bit)

| Range | Contents |
| --- | --- |
| 0x00_0000_0000 to 0x03_FFFF_FFFF | GDDR low (this card: 6 GB used, rest unpopulated) |
| 0x00_FEE0_0000 | Local APIC (relocatable) |
| 0x00_FF00_0000 to 0x00_FFFF_FFFF | Boot code (flash) and fuses via SBOX |
| 0x04_0000_0000 to 0x07_FFFF_FFFF | GDDR high (up to PHY_GDDR_TOP) |
| 0x08_007C_0000 to 0x08_007C_FFFF | DBOX registers (64 KiB) |
| 0x08_007D_0000 to 0x08_007D_FFFF | SBOX registers (64 KiB) |
| 0x0C_0000_0000 to 0x0F_FFFF_FFFF | Reserved |
| 0x10_0000_0000 to 0x7F_FFFF_FFFF | Reserved |
| 0x80_0000_0000 to 0xFF_FFFF_FFFF | System (host) address range: 32 windows of 16 GiB, translated by the SMPT |

## SMPT (System Memory Page Table)

32 registers at SBOX offset 0x3100 (`SBOX_SMPT00`), 4 bytes each. Entry `i`
maps card physical `0x80_0000_0000 + i * 2^34` to host address
`(entry[31:2]) << 34`. Bit 0 set means no-snoop. Intel's SCIF never set
no-snoop (SSDG 2.1.12), so host accesses are always coherent.

Host-side view with an IOMMU: the "host address" written into an entry is an
IOVA in the VFIO container, not a physical address. This project maps window
`i` to IOVA `i << 34` so card address and IOVA differ only by the constant
`0x80_0000_0000`. The allocator never hands out IOVA pages inside the two
reserved ranges reported for IOMMU group 30 on this host:
`0xfee00000-0xfeefffff` (MSI) and `0xfd00000000-0xffffffffff`.

## Host view of the card

| BAR | Name | This host | Contents |
| --- | --- | --- | --- |
| BAR0 | MEMBAR0, the aperture | 0x7c00000000, 16 GiB, prefetchable | Card physical memory starting at `APR_PHY_BASE` (default 0). Offset `x` in BAR0 is card physical `x`. GDDR is 6 GB, so offsets above that are unpopulated. |
| BAR4 | MEMBAR1 | 0xfce00000, 128 KiB | DBOX at offset 0, SBOX at offset 0x10000. The POST code register is at BAR4 offset 0x242c (DBOX side, read directly, mainline `mic_x100_get_postcode`). |

Host CPU accesses through BAR0 are uncached MMIO writes over PCIe: adequate
for loading a boot image (the built one is 8.4 MB of bzImage plus 1.4 MB of
initramfs, a few seconds) and for the console ring, not for bulk data.
Bulk data uses the card's DMA engine, built on 2026-09-16
(`docs/results/2026-09-16-dma.md`, 3.58 GB/s memory to memory against the
aperture's tens of MB/s).

## Coherence

- Host accesses to card cacheable memory are snooped by the card (SSDG
  2.1.12). The card kernel can therefore map the ring region cacheable.
- Card accesses to host memory are snooped by the host unless no-snoop is set.
- Host CPU mappings of BAR0 through VFIO are uncached, so no host-side cache
  state exists to worry about. Posted-write ordering across PCIe still
  requires an MMIO read-back before signaling the card.
