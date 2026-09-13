# phi-regs / postcode.rs

POST codes are the only progress signal available before the console ring
exists.

## Encoding (measured)

The register holds two ASCII characters in its low two bytes, low byte
first. Intel's table (MPSS 2.1 readme) lists codes as mixed-case strings
(`"0b"`, `"3d"`, `"dE"`), which only makes sense for characters, and the
first read on this card (2026-09-13) returned `0x6330`, the pair `"0c"`.
The earlier assumption that the low byte was a small integer was wrong and
is corrected here; `docs/results/2026-09-13-first-contact.md` has the raw
values.

## Resolved by the reset trace (`docs/results/2026-09-13-reset-2.md`)

The register is live. A reset walks `"0c"`, the GDDR training codes
`"30"`..`"3F"` (7.4 s), an undocumented `"16"`, `"09"`, `"0F"`, `"10"`,
and ends at `"12"` after 9.3 s. The characters match Intel table mixed
case exactly, so the table was transcribed from this register. `"16"` is
decoded as "undocumented" with the observation date.
