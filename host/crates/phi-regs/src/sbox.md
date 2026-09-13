# phi-regs / sbox.rs

The SBOX/DBOX register subset this project touches. The human-readable table
with the same content is `docs/spec/sbox-registers.md`; this file is the
machine-readable version and the tests pin the derived offsets to literal
values from Intel's driver.

## Facts the code depends on

- BAR4 is 128 KiB; DBOX occupies the first 64 KiB, SBOX the second
  (SSDG 2.1.12, and `MIC_X100_SBOX_BASE_ADDRESS = 0x10000` in `mic_x100.h`).
- `POSTCODE` (0x242c) is read relative to BAR4 itself, not the SBOX base.
  This is easy to get wrong; `mic_x100_get_postcode` in v5.9 is the proof.
- Scratchpad 2 carries the bootstrap's download address (bits 31:12), the
  BSP APIC ID (bits 9:1) and the ready flag (bit 0). Scratchpad 5 receives
  the image size. Both from `mic_x100.h`/`mic_x100.c`.
- The boot interrupt is vector 229 through `APICICR7`, with bit 13 set in
  the low dword to trigger delivery, after writing the APIC ID to the high
  dword (`mic_x100_send_firmware_intr`).

## What is deliberately absent

DMA engine registers, thermal and power registers, GDDR configuration.
They come from `micsboxdefine.h` in `vendor/mpss-3.8.6` when a phase needs
them, with the same citation discipline.

## Testing

`cargo test -p phi-regs`: offset derivations, field decoding, and a check
that every offset lies inside BAR4.
