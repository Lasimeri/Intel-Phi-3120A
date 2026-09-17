# phi-regs / memory.rs

Card-side physical addresses and sizes, plus the SMPT entry encoding.

## Sources

- SSDG 2.1.12 for the card physical map (DBOX, SBOX, LAPIC, flash, the
  512 GiB system range).
- `intelmic.c` (`MIC_SBOX_BASE`, `BUILD_SMPT`, `MIC_SYSTEM_PAGE_SHIFT`) and
  `mic_x100.c` (`mic_x100_smpt_set`, `mic_x100_smpt_hw_init`) for the SMPT
  entry format, base and count.
- `docs/hardware.md` for the measured 16 GiB aperture.
- Intel ARK for the 6 GB GDDR capacity of the 3120A.
- `docs/results/2026-09-13-first-contact.md` for the 64 MiB download
  address the bootstrap reports in `SPAD2`.

## The ring region default

256 MiB (`0x1000_0000`) is a choice, not a documented fact, and the
reasoning is in `docs/spec/ring-protocol.md`: the bootstrap asks for the
kernel at 64 MiB, `phi-hw` places the initramfs at twice that (128 MiB,
as Intel's loader did), and nothing of Intel's ever wrote below the
download address. The region was first placed at 32 MiB, below the
download address, and bulk writes there reset the host
(`docs/results/2026-09-13-p3-kernel-build.md`); the compile-time assertion
keeps it at or above 128 MiB and inside the GDDR. The card kernel reserves
the region with `memmap=` so it never becomes page cache; `phictl boot
--ring-base` overrides the constant at runtime.

## Testing

`cargo test -p phi-regs`: the SMPT encoding (page number in bits 31:2,
no-snoop in bit 0, low address bits dropped) and consistency of the map
constants with each other.
