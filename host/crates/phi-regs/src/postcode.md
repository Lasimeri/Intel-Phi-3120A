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

## Codes written by the card kernel

The card kernel from `card/kernel/patches` writes its own progress codes
into the same register (`asm/knc.h` in patch 0010): the letter `K`
followed by one character, so they never collide with Intel's numeric
codes. `K0` and `K1` come from the decompressor, `K2` to `K5` from the
platform layer inside `setup_arch()`, `K6` from the ring console, `K7`
from a late initcall just before `init` starts, and `KH`, `KP`, `KE` mark
halt, panic and the fatal "no SFI tables" case. `phictl postcode` decodes
them like Intel's; the decoder table lists every one.
