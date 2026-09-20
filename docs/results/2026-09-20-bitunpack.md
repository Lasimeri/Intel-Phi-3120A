# 2026-09-20: bit-unpacking on the vector unit, 46 to 58x

The gate measurement. Every compression result in this project so far has
been a loss (xz 0.22 to 0.26x the host, zstd 0.03 to 0.07x), and the
reading from `docs/research/compression-on-knc.md` was that the card should
win on columnar integer codecs for structural reasons. This tests that
prediction with one kernel.

It holds, by a wider margin than expected.

## Measured

`card/examples/bitunpack.c` with `bitunpack.S`, one thread, 1024-value
blocks, values compared against the originals before anything is timed.

| bits | pass | stream M/s | lanes M/s | vpu M/s | vs stream |
| --- | --- | --- | --- | --- | --- |
| 5 | hot | 47.8 | 125.5 | **2259.3** | **47.2x** |
| 5 | streaming | 42.0 | 98.9 | 370.3 | 8.8x |
| 11 | hot | 42.9 | 118.1 | **1972.2** | **46.0x** |
| 11 | streaming | 38.8 | 91.4 | 320.7 | 8.3x |
| 16 | hot | 51.8 | 131.0 | **2985.6** | **57.6x** |
| 16 | streaming | 45.3 | 96.2 | 305.7 | 6.8x |

`stream` is scalar over a contiguous bitstream, the ordinary layout.
`lanes` is scalar over the interleaved layout. `vpu` is `knc_unpack_bN`.
`hot` keeps one block in L1 and measures instruction throughput;
`streaming` walks 16 MiB of output and measures memory.

## Three readings

**The card's vector unit is worth 46 to 58x its scalar code on this shape
of work.** Nothing else in the project comes close: the Mandelbrot kernel
gets 1.47x over the *host's* AVX2, the VPU memcpy 1.11 to 1.63x over
musl. The difference is that bit-unpacking is entirely 32-bit lane
arithmetic with compile-time-constant shift counts, no control flow and no
cross-lane movement, which is the exact shape the prediction named.

**The interleaved layout is worth 2.5x before any vector code.** `lanes`
runs at 118 to 131 M/s against `stream`'s 43 to 52, because the shift
counts become loop invariants and the inner loop is sixteen identical
operations.

**Correction.** The first version of this document said the opposite, that
the layout was a loss at 24 to 27 M/s. That was a bad baseline, not a
result: the `lanes` implementation looped over values and recovered the
lane and position with a divide and a modulo per value, which measures the
traversal rather than the layout. Rewriting it position-outer, lane-inner,
which is how FastLanes writes it, moved it to 118 M/s against identical
data. The `vpu` and `stream` columns were unaffected and are unchanged, so
the headline 46 to 58x is as it was; the `vs lanes` ratio drops from about
80x to about 17x. `card/examples/bitunpack.c` carries the fixed version.

**Scheduling was worth 1.8x on its own.** The first version of the
generator emitted one value's whole dependency chain before starting the
next and reached 1.07 values per cycle, roughly fifteen cycles for a
four-instruction chain. Emitting phase by phase across eight values, so
consecutive instructions are never dependent, took it to 1.79 with the
same instruction count. The core is in order and there is no compiler
between the generator and the card, so instruction order is the
generator's job. Recorded because it is the general lesson for every
kernel added here, not a detail of this one.

## What this does not say

It does not say the card beats the host. Every number is one thread of 228,
and there is no host-side comparison here.

All three of those were done later the same day and are in
`docs/results/2026-09-20-libknc.md`: all 32 widths, both directions, 228
threads, and the host running the same layout through its own
auto-vectoriser. Short version, at 11 bits: the card reaches 12556 M values
per second against the host's 30836, or 0.41x, and it is then sitting on
its memory bandwidth ceiling.

## Method notes

A single-thread issue ceiling sits just above these numbers. 1.79 values
per cycle at 11 bits is 128 values per about 30 instructions, and KNC
issues at most one instruction every two cycles from a single thread, so
one thread is near its limit here while one core is not.

Widths 5, 11 and 16 were chosen to separate the cases: 11 straddles word
boundaries (the general case), 5 is narrow, 16 never straddles. The
generator is parameterised by width; the rest are one line each and were
left ungenerated because three answered the question.
