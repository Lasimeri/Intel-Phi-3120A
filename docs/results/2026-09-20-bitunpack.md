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
| 5 | hot | 47.8 | 26.1 | **2257.1** | **47.3x** |
| 5 | streaming | 41.9 | 24.6 | 360.2 | 8.6x |
| 11 | hot | 42.8 | 24.5 | **1970.9** | **46.0x** |
| 11 | streaming | 38.7 | 23.1 | 312.8 | 8.1x |
| 16 | hot | 51.7 | 27.2 | **2985.1** | **57.7x** |
| 16 | streaming | 45.1 | 25.2 | 294.5 | 6.5x |

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

**The interleaved layout on its own is a loss.** `lanes` is consistently
slower than `stream` in scalar, 24 to 27 M/s against 42 to 52. The layout
costs scalar code a multiply and a strided access per value and buys it
nothing. FastLanes reports the opposite on modern hardware, where the
compiler auto-vectorises the scalar form; nothing auto-vectorises to MVEX
here, so on this card the layout is purely a vector-unit enabler and is
worth adopting only together with hand-written kernels.

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

It does not say the card beats the host. Every number is one thread of
228, and there is no host-side implementation of the same layout to
compare against. The streaming column is the one that matters for a real
codec and it is bandwidth-bound, which is the good case for this card
(80.5 GB/s at 114 threads against the host's 35.7), but that is an
argument, not a measurement.

The honest next steps, in order: the same kernel on 114 and 228 threads,
then an AVX2 implementation of the same layout on the host, then the
remaining bit widths.

## Method notes

A single-thread issue ceiling sits just above these numbers. 1.79 values
per cycle at 11 bits is 128 values per about 30 instructions, and KNC
issues at most one instruction every two cycles from a single thread, so
one thread is near its limit here while one core is not.

Widths 5, 11 and 16 were chosen to separate the cases: 11 straddles word
boundaries (the general case), 5 is narrow, 16 never straddles. The
generator is parameterised by width; the rest are one line each and were
left ungenerated because three answered the question.
