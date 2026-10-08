# sensors.S: the SensorsReply text

`sensors_frame` builds the SensorsReply the relay (`serve.S`,
`handle_frame`) answers a client's Sensors request with, in `tx_buf`
through `f_begin`: the text the Rust daemon built (`serve.rs`
`sensors_text`) from the SBOX through the decoders of `phi-regs`
`sbox::sensors`, word for word. It was part of `serve.S` until
2026-10-08; nothing in it touches the relay's state, only the card's
registers through `sbox_read` and `spad` (`card.S`).

What the text holds, each register named in `defs.inc` with its source:

- the nine die temperatures (`CURRENT_DIE_TEMP0` and the two registers
  after it, three 10-bit fields per register, 0 shown as n/a) and the
  highest of the nine maxima (`MAX_DIE_TEMP0` and following);
- the board temperatures (`BOARD_TEMP1`, `BOARD_TEMP2`: inlet, the VCCP
  regulator, GDDR, the GDDR regulator, each half valid when its bit 15
  is set, `w_opt_temp`), the VDDG regulator field of `STATUS_FAN2` (bits
  19:12), the TMU die temperature of `THERMAL_STATUS` (bits 30:22, valid
  with bit 31);
- the VR12 core voltage (`COREVOLT`: 250 mV plus 5 mV per code above 1);
- the core clock from `COREFREQ` and `CURRENT_CLK_RATIO` through Intel's
  PLL table (`core_khz`, `w_khz`: feedback bits 8:1 times 200 MHz over
  the feed-forward divider of bits 10:9 inverted, feedback 8..16 for
  divider 1 and 8..15 for 2 and 4, scaled by 20 over the ICC divider of
  SPAD4 bits 29:25, 20 when unfused; a code outside the table prints as
  "n/a (code 0x...)").

Verified against the Rust daemon's output on card 1 (`phictl sensors`,
`docs/results/2026-09-29-phictl-assembly.md`).
