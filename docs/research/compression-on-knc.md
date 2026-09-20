# Which compression algorithms suit Knights Corner

Question: after xz (0.22 to 0.26x the host) and zstd (0.03 to 0.07x), is
there a compression algorithm this card is actually good at?

Answer: yes, but not in the LZ77 family. The card wants **columnar integer
codecs and block float codecs**, because those are the only ones whose
inner loop is wide, regular 32-bit lane arithmetic. Two specific
candidates, `FastLanes` and `zfp`, need exactly the instruction subset
KNC has and nothing it lacks.

Surveyed 2026-09-20. Nothing below has been run on the card yet; this is
the reading that decides what to port next.

## The filter every candidate has to pass

Established in `docs/results/2026-09-20-zstd-vpu.md`, from the ISA
reference (327364-001): KNC has **no byte or word integer vector
instructions at all**. Every integer vector operation is `D` (32-bit
lanes) or `Q` (64-bit). Concretely, what exists:

| Available | Missing |
| --- | --- |
| `VPADDD`, `VPSUBD` | `vpaddb`, `vpaddw` |
| `VPSLLD`, `VPSRLD`, `VPSLLVD`, `VPSRLVD` | any byte or word shift |
| `VPANDD/Q`, `VPORD/Q`, `VPXORD/Q` | |
| `VPCMPEQD`, `VPCMPGTD`, `VPCMPD`, `VPCMPUD` | `vpcmpeqb` |
| `VPSHUFD` (32-bit lane permute) | `pshufb` (byte shuffle) |
| `vmovaps`/`vmovapd`, `vloadunpackld/hd` | single-instruction unaligned load |
| mask registers, `KMOV`/`KAND`/`KOR`/`KORTEST` | |

So the test is: **does the codec's inner loop work on 32-bit or wider
lanes using only shift, and, or, xor, add, compare and a lane-granular
permute?** If it needs `pshufb`, it is out.

## Ruled out, and why

| Codec | Blocked by |
| --- | --- |
| zstd, LZ4, Snappy, Brotli, gzip | Byte-oriented LZ77. Match comparison, literal handling and history copy are all byte work. Measured: zstd 0.03 to 0.07x |
| xz / LZMA | Serial range coder plus pointer-chasing match finder; nothing to vectorise. Measured 0.22 to 0.26x |
| **StreamVByte**, varint-G8IU | Decode is a `pshufb` on a shuffle mask, by construction. This is the whole trick of the format |
| Blosc `shuffle` / `bitshuffle` filters | Byte and bit transposition via SSE2/AVX2 byte shuffles |

StreamVByte deserves emphasis because it looks like a fit on paper: it is
an integer codec, and integers are 32-bit. It is not. Its decode step is
"apply `pshufb` with a mask selected by the control byte", so the byte
shuffle *is* the algorithm.

## Candidate 1: FastLanes

The strongest match, and the match is close to exact.

`FastLanes` (Afroozeh and Boncz, VLDB 2023) exists specifically to make
bit-packing portable across SIMD widths and instruction sets. It defines
a virtual 1024-bit register `FLMM1024` and an interleaved "Unified
Transposed Layout" that is the same for every lane width, so the same
layout decodes on 128-bit NEON, 512-bit AVX-512, or scalar registers just
by issuing more or fewer identical instructions.

The paper states the requirement directly:

> In order to support heterogeneous ISAs, FastLanes only uses simple
> operators, such as load/store, left/right-shift, and/or/xor, addition
> and set instructions; supported for all lane-widths, T in {8, 16, 32, 64}

**Every one of those exists on KNC at T = 32 and T = 64.** No shuffle, no
permute, no cross-lane movement, which is exactly the design goal that
makes it fit. T = 8 and T = 16 are unavailable here, so 8-bit and 16-bit
columns would fall back to scalar; 32-bit and 64-bit columns are the
interesting ones anyway.

It covers the lightweight encodings that matter for columnar data:
bit-packing, FOR (frame of reference), delta, dictionary and RLE, with
cascading. MIT licensed, C++ with Python and Rust bindings.

**The catch, which no paper mentions because it is specific to this
card.** FastLanes deliberately uses *no intrinsics*: it is scalar C++ that
relies on the compiler to auto-vectorise. Our patched LLVM has no KNC
vector backend at all, so it will never auto-vectorise to MVEX. Compiled
for the card as-is, FastLanes runs as scalar code.

That is not fatal, and it cuts both ways:

- The paper notes scalar FastLanes still gains from packing several small
  values into a 64-bit register ("8-bits gets 8x faster using 64-bits
  scalar"), so a straight port should already beat a naive codec.
- Because the operator set is exactly KNC's subset, hand-writing the
  decode kernels in MVEX through `knc-mvex` is a bounded job rather than a
  research problem. This is the one place where the project's existing
  encoder is pointed at precisely the right target.

Reported performance on modern hardware, as an upper bound to calibrate
against: 1024 values decoded in as little as 17 cycles, about 70 values
per core cycle.

## Candidate 2: zfp

The float-side answer, and a better structural fit than it first looks.

`zfp` (Lindstrom, LLNL) compresses d-dimensional float arrays in
independent blocks of 4^d values. A 2D block is 16 values, which for
`float32` is exactly 512 bits: **one zmm register per block.** In 3D, 64
values is four zmm of float32 or eight of float64.

Its inner operations, from the algorithm documentation:

| Stage | Operations |
| --- | --- |
| Block transform (lifting, in place) | 2.5d integer **additions** and 1.5d **single-bit shifts** per integer |
| Two's complement to negabinary | one **addition** and one **xor** per integer |
| Coefficient reordering | a fixed, compile-time-known permutation (zig-zag style) |
| Embedded bit-plane coding | shifts, masks and ors to gather bit planes |

Adds, shifts, xor, and, or, on 32-bit or 64-bit integers derived from
floats, plus one fixed permutation that `VPSHUFD` can express because it
is lane-granular rather than byte-granular. **Every stage maps onto
instructions KNC has.**

It is also the right shape for what this card is: a machine whose vector
unit is float-oriented, with 16 float32 or 8 float64 lanes, that already
beat the host's AVX2 by 1.47x on float64 arithmetic
(`docs/results/2026-09-15-vpu.md`). zfp is lossy, which narrows the use
case to scientific and numeric arrays, and `fpzip` is the lossless
relative if that matters.

## Candidate 3: SIMD-BP128 and SIMD-FastPFOR

Lemire's vectorised integer codecs, the direct ancestors of FastLanes.
They operate on blocks of 128 unsigned 32-bit integers across four
SIMD-friendly lanes, using shifts and masks; the published
implementations use SSE2 intrinsics.

Compatible with KNC in principle, and the arithmetic is the right shape.
Superseded by FastLanes for this purpose: the 4-way layout was designed
for 128-bit registers and, as the FastLanes authors note, "does not have
enough parallelism for 256-bits or 512-bits registers". On a 512-bit
machine you would be reimplementing the interleaving anyway, which is the
problem FastLanes already solved.

## Nobody has published compression numbers on this hardware

Searching for measured compression work on Knights Corner or MIC turns up
molecular dynamics, breadth-first search, polyphase filters, adaptive
optics and MapReduce, but no compression study. The Xeon Phi literature
is overwhelmingly about floating-point throughput, which is what the part
was sold for.

So there is no prior result to compare against, and a measured
bit-packing or zfp number on a 3120A would be new. Treat that as a
caution as much as an opportunity: it may be unpublished because it is
uninteresting, and the honest prior from this project's own measurements
is that the card loses at almost everything.

## What the project's own numbers predict

| Workload | Card against host | Shape |
| --- | --- | --- |
| Mandelbrot on the VPU | **1.47x** | float64, 8 regular lanes, no control flow |
| Memory read | **2.25x** | streaming |
| VPU block copy | 1.11 to 1.63x | lane-agnostic, bandwidth-bound |
| xz | 0.22 to 0.26x | branchy integer, serial coder |
| zstd | 0.03 to 0.07x | byte-serial, cheap per byte |

The card wins where work is wide, regular, and at least 32 bits per lane,
and loses where it is byte-serial with data-dependent control flow.
Bit-packing and zfp sit firmly in the first column: fixed-width lanes,
no branches in the inner loop, arithmetic rather than byte shuffling.
That is the argument for expecting a different result, and it is an
argument from structure, not from measurement.

## This was pursued, the same day

The plan below was written on 2026-09-20 and executed on 2026-09-20. What
happened, against what was predicted:

| Step | Outcome |
| --- | --- |
| Extend `knc-mvex` with the integer operators | Done, eleven of them, verified on the card: `docs/results/2026-09-20-mvex-integer.md` |
| Hand-write one bit-unpacking kernel and measure | Done, 46 to 58x the card's scalar code: `docs/results/2026-09-20-bitunpack.md` |
| Everything after that | `libknc`: all 32 widths, both directions, FOR and DELTA, five language bindings |

The prediction in this document was right about the structure and wrong
about one number. It said the card would win where work is wide, regular
and at least 32 bits per lane, and it does: 46 to 58x its own scalar code
in cache. It did not anticipate that at 228 threads the card is
memory-bound, so the vector unit's margin over scalar collapses from 4.6x
per thread to 1.4x at full occupancy, and the card lands at 0.28 of the
host rather than ahead of it (`docs/results/2026-09-20-fastlanes.md`).

FastLanes itself was not built for the card. The paper's implementation
relies on auto-vectorisation, and nothing auto-vectorises to MVEX, so
building it would have measured the scalar path and nothing else. The
layout was implemented directly instead, at 512 bits; `knc.md` says how it
differs from the published Unified Transposed Layout and why that
difference is deliberate.

zfp is still untried, and is still the obvious next candidate: the same
argument, on the float side, where this card's vector unit is at its
strongest.

## Sources

- Afroozeh and Boncz, *The FastLanes Compression Layout: Decoding over 100
  Billion Integers per Second with Scalar Code*, PVLDB 16(9), 2023:
  <https://www.vldb.org/pvldb/vol16/p2132-afroozeh.pdf>
- FastLanes implementation: <https://github.com/cwida/FastLanes>
- Lemire and Boytsov, *Decoding billions of integers per second through
  vectorization*: <https://arxiv.org/abs/1209.2137>
- Lemire, Kurz and Rupp, *Stream VByte: Faster Byte-Oriented Integer
  Compression*: <https://arxiv.org/pdf/1709.08990>
- zfp algorithm documentation:
  <https://zfp.readthedocs.io/en/release1.0.1/algorithm.html>
- zfp compression ratio and quality, LLNL:
  <https://computing.llnl.gov/projects/zfp/zfp-compression-ratio-and-quality>
- C-Blosc, shuffle and bitshuffle filters:
  <https://github.com/Blosc/c-blosc>
- Intel Xeon Phi, architecture summary:
  <https://en.wikipedia.org/wiki/Xeon_Phi>
