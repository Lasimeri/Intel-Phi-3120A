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
decoded as "undocumented" with the observation date. During the reset the
card drops off the bus for one sample and the register reads
`0xffffffff`, which `text()` shows as hex.

## Codes written by the card kernel

The card kernel from `card/kernel/patches` writes its own progress codes
into the same register (`KNC_POST(a, b)` in `asm/knc.h`, patches 0010 and
0012): a letter followed by one character, so they never collide with
Intel's numeric codes.

- `K0`/`K1` from the decompressor; `K8`, `KF`, `KG`, `KJ`..`KN` from
  `startup_64` (raw `movl` constants in `head_64.S`); `KO`, `KQ`, `KR`,
  `K9` from the first C code; `KA`..`KD` from `setup_arch`; `K2`..`K5`
  from the platform layer; `K6` from the ring console; `K7` from a late
  initcall just before `init` starts; `KH`, `KP`, `KE` mark halt, panic and
  the fatal "no SFI tables" case.
- `S0`..`S6` from the boot CPU around INIT and SIPI, and `A1`..`A8` from
  each application processor between the trampoline and idle (patch 0019,
  `docs/results/2026-09-14-p4-tty-smp.md`).

`phictl postcode` decodes them like Intel's; the decoder table lists every
one, and the test checks a sample of each family against the patch
constants.
