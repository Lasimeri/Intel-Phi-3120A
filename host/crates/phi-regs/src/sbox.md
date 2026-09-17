# phi-regs / sbox.rs

The SBOX/DBOX register subset this project touches, with bit layouts and
the decoders that turn raw words into numbers. The human-readable table
with the same content is `docs/spec/sbox-registers.md`; this file is the
machine-readable version and the tests pin the derived offsets to literal
values from Intel's headers and the decoders to values measured on this
card.

## Facts the code depends on

- BAR4 is 128 KiB; DBOX occupies the first 64 KiB, SBOX the second
  (SSDG 2.1.12, and `MIC_X100_SBOX_BASE_ADDRESS = 0x10000` in `mic_x100.h`).
- `POSTCODE` (0x242c) is read relative to BAR4 itself, not the SBOX base.
  This is easy to get wrong; `mic_x100_get_postcode` in v5.9 is the proof.
- Scratchpad 2 carries the bootstrap's download address (bits 31:12), the
  BSP APIC ID (bits 9:1) and the ready flag (bit 0). Scratchpad 5 receives
  the image size. Both from `mic_x100.h`/`mic_x100.c`. Scratchpad 4 is the
  platform word (`sboxScratch4RegDef` in `intelmic.c`): thread mask, L2
  size, memory channel count, fused ICC divider.
- The boot interrupt is vector 229 through `APICICR7`, with bit 13 set in
  the low dword to trigger delivery, after writing the APIC ID to the high
  dword (`mic_x100_send_firmware_intr`).
- The core clock is `reference * feedback / feedforward`, reference being
  4000 MHz over the ICC divider from scratchpad 4 (`get_core_freq` in
  `intelmic.c`), the ratio word being bits 11:0 of `COREFREQ` or of
  `CURRENT_CLK_RATIO`. `sensors::core_khz` adds Intel's range table
  (`cpu_tab` in `micras_knc.c`) and the RAS rule that a divider of 0 means
  the nominal 20; `PlatformInfo::reference_mhz` uses the same rule so the
  two decoders agree (a test walks the whole table).

## KNF versus KNC offsets

MPSS 3.8.6's `include/mic/micsboxdefine.h` is a merged KNF/KNC list.
Some registers moved between the two generations and the header carries
both values, either under `#ifdef CONFIG_MK1OM` (`COREFREQ`, `COREVOLT`,
`SDBIC0`) or with a `_K1OM` suffix (`MXAR0`, `MSIXPBACR`), and in one case
with the KNF value unguarded: `SBOX_CURRENT_CLK_RATIO = 0x3004`. The
KNC-only header in Intel's k1om kernel (`mic_knc/micsboxdefine.h`) has
no register at 0x3004 and names the live ratio `SBOX_CURRENTRATIO =
0x402C`; `intelmic.c` uses 0x402C under `CONFIG_MK1OM` and 0x3004 in its
KNF branch. This crate carried 0x3004 until 2026-09-17, which is why the
sensors record of 2026-09-16 says the register "reads 0 on this card".
Read on the card the same day (`devmem` through `phictl exec`):
SBOX+0x402C = 0x04160416 (current and goal ratio both 0x416, 1100 MHz),
SBOX+0x3004 = 0. The full read is recorded in
`docs/spec/sbox-registers.md`. Rule for future additions: take offsets
from the `mic_knc` header first and treat the merged MPSS header as a
KNF list unless a `CONFIG_MK1OM` guard or `_K1OM` suffix says otherwise.

## Sensors

`sensors` decodes the die, board, VDDG and TMU temperatures, the core
voltage (VR12 SVID: 250 mV plus 5 mV per step above code 1) and the core
clock exactly as `mr_get_temp`, `svid2volt` and `mr_mt_cf_r2f` in MPSS
`ras/micras_knc.c` do. The card's hwmon driver (kernel patch 0027) is the
C twin of this module. The board, VDDG and TMU fields depend on SMC
telemetry broadcasts that only Intel's I2C driver requests, so on this
port they read 0 with the valid bits clear (`docs/results/2026-09-16-sensors.md`).

## DMA engine

`DCR` and the eight channel blocks at `0xA000 + 0x40 n` come from
`micsboxdefine.h`; the `DRAR_HI` bit fields, the interrupt mask bits and
the descriptor layouts (`memcopy` type 1 with the length in 64-byte lines
in bits 59:46, `status` type 2) from `dma/mic_dma_md.c` and
`include/mic/mic_dma_md.h`. `dma_memcpy_desc` refuses transfers over
`MIC_MAX_DMA_XFER_SIZE` (1 MiB minus one line); `phi-hw` splits larger
copies.

## Testing

`cargo test -p phi-regs`: offset derivations against literal defines, a
check that every offset lies inside BAR4 on a dword boundary, and the
decoders against words measured on this card (scratchpads 2 and 4 from
2026-09-13, `CURRENTRATIO`, `COREFREQ`, `COREVOLT` and the die
temperature registers from 2026-09-17) plus synthetic words that exercise
every field and valid bit.
