# phi-regs / memory.rs

Card-side physical addresses and sizes, plus the SMPT entry encoding.

## Sources

- SSDG 2.1.12 for the card physical map (DBOX, SBOX, LAPIC, flash, the
  512 GiB system range).
- `intelmic.c` (`MIC_SBOX_BASE`, `BUILD_SMPT`, `MIC_SYSTEM_PAGE_SHIFT`) for
  the SMPT entry format; `mic_x100_smpt_hw_init` for the base and count.
- `docs/hardware.md` for the measured 16 GiB aperture.
- Intel ARK for the 6 GB GDDR capacity of the 3120A.

## The ring region default

32 MiB is a choice, not a documented fact. The reasoning: the bootstrap
reports a download address of 64 MiB in Intel's experience (comment in
`mic_x100_load_ramdisk`), places the AP boot code somewhere in low memory
(POST code 0x0E), and the initramfs goes at 128 MiB. 32 MiB to 33 MiB is
therefore unclaimed by anyone. The kernel reserves it with `memmap=` so it
never becomes page-cache. If phase P1 shows a different download address
the constant moves; `phictl boot --ring-base` overrides it at runtime.
