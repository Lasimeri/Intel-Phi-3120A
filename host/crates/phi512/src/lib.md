# phi512

Lets a program built for AVX-512 run on a machine that has none.

```sh
scripts/phi512.sh ./my-avx512-program
```

The program is not modified, not recompiled, and not aware of any of this.
It executes an AVX-512 instruction, the processor refuses it, and the
library performs the instruction and lets the program carry on.

## The register file is imaginary, and that is the whole trick

This host has no `zmm` registers at all. There is nowhere to put the values
the program believes it is computing with, so they live in an ordinary
array instead (`state.md`).

That works only because the illusion is **complete**. A program cannot read
a `zmm` register except through an AVX-512 instruction; every one of those
faults; every one of those is serviced here. Nothing else in the process
can tell the difference.

It also means a gap is fatal rather than cosmetic. If one AVX-512
instruction were skipped, or performed slightly differently, the imaginary
register file would disagree with what the program believes it computed,
and every answer after that would be wrong with no indication. So anything
unrecognised stops the program with a message naming the instruction,
rather than being passed over.

## What it costs today, and why that is not where it stays

Measured on this host: **2101 ns per AVX-512 instruction**, of which the
fault is 1909 ns. The arithmetic is under 200 ns; **91 percent of the cost
is the processor telling us the instruction happened.**

That is the shape of the thing to fix, and it is fixable. An EVEX
instruction is at least 6 bytes and a near jump is 5, so a site can be
overwritten in place with a jump to its translation the first time it
faults, and never fault again. Measured targets for the stages after that,
from `docs/research/avx512-transparency.md`:

| stage | cost against native AVX2 |
| --- | --- |
| today: fault on every execution | about 2000x |
| patch each site, translate one instruction at a time | 6.54x |
| patch each site, translate whole regions | 1.11x |
| hot loops sent to the card's 57 vector units | see `docs/results/2026-09-21-avx512-translation.md` |

The last two rows are measured, not estimated; what does not exist yet is
the patcher and the region finder.

## Why the emulator is written in scalar Rust

`emulate.md` covers this, but the short version: these are IEEE operations,
so scalar arithmetic gives the same bits as vector arithmetic, and it is
far easier to be sure of. Speed is meant to come from not reaching this
code at all, rather than from making it clever.

The one place that is not a free choice is the fused multiply-add, which
uses `mul_add` because it must round once. Writing `a * b + c` would round
twice and produce different bits from the hardware.

## Limits worth knowing

- The imaginary register file is per thread, which is right, but it starts
  zeroed in each thread rather than being inherited, which matches how a
  thread's vector state actually begins.
- The instruction table is a useful subset of AVX-512F, not all of it.
  `emulate::supported` is the list.
- Gather, scatter, and the mask-register instructions (`kmov` and friends)
  are not implemented yet.
- This is the host path. It does not use the card; `avx512-xlate` is the
  piece that targets the card, and connecting the two is future work.

## Modules (2026-09-22)

`offload` is the seamless path to the card (the product); `emulate`,
`patch` and `frame` are the software fallback behind `PHI512_EMULATE`;
`state` is the vector register file both share; `handler` chooses.
