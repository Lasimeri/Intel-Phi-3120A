# Running AVX-512 on a machine that has none

2026-09-21. The host is a Ryzen 7 5800X: AVX2 and FMA3, and no AVX-512 of
any kind. The card is a Xeon Phi 3120A, whose vector unit predates AVX-512
and speaks a different encoding. The question was whether the card can
execute AVX-512 on the host's behalf, and produce the bits AVX-512 hardware
would have produced.

It can. A translated AVX-512 kernel runs on the card at **0.97x the speed
the host reaches with its own native vector unit**, bit-identical
throughout, on an instruction set the host cannot execute at all.

## The gating question was precision, not opcodes

An emulator that is fast and wrong is worth nothing, so the first work was
an audit of 327364-001 rather than any code. The full evidence is in
`docs/research/avx512-on-knc.md`. The summary:

| property | verdict |
| --- | --- |
| add, subtract, multiply, float32 and float64 | bit-exact, `MXCSR.DAZ`/`FZ` exactly as AVX-512 |
| all twelve fused multiply-add forms | bit-exact, fused and single-rounded on both |
| the four rounding modes | 1:1, dynamic through `MXCSR.RC` and per-instruction |
| suppress-all-exceptions | 1:1 |
| mask register width | 16 bits, which is what AVX-512F uses |
| divide and square root | **absent from the card entirely** |
| minimum and maximum | different NaN rule, two instructions to fix |
| reciprocal approximations | AVX-512 defines only an error bound, so there are no reference bits to reproduce |

The two surprises were both in the card's favour. MVEX static rounding is
the direct ancestor of EVEX embedded rounding, so `{er}` and `{sae}` map one
to one rather than needing emulation. And `VPERMD` is present, which an
earlier guess in this project had wrong.

The one real hole is that `vdivps`, `vdivpd`, `vsqrtps` and `vsqrtpd` do not
exist on Knights Corner. Not sparse: the strings `vdiv` and `vsqrt` appear
nowhere in the manual.

## Knights Corner is x86-64, so most of a function needs no translation

The scalar half of a vectorised function, the pointer arithmetic, the loop
counter, the compare, the branch, is already valid card code. Only the EVEX
instructions have to change. On the polynomial kernel that is 264
instructions rewritten and 7 left alone.

And for the arithmetic that exists on both machines, the rewrite touches
two bits. EVEX and MVEX are both a `62H` prefix and three payload bytes.
They differ at bit 2 of P1, which EVEX fixes at 1 and MVEX does not use,
and in P2, where EVEX packs `z|L'L|b|V'|aaa` and MVEX packs `E|SSS|V'|aaa`.
So `vaddps zmm0, zmm1, zmm2` is `62 f1 74 48 58 c2` as AVX-512 and
`62 f1 70 08 58 c2` on the card.

The front of the pipeline works because the host assembler will happily
encode instructions the host processor cannot run. `llvm-mc` assembles the
AVX-512 source; `iced-x86` decodes it; `avx512-xlate` rewrites the EVEX;
`knc-mvex` encodes the result. The gap between what this host can encode
and what it can execute is the whole opportunity.

Instruction lengths change during the rewrite, because an unaligned AVX-512
load becomes two card instructions, so branch displacements cannot be
copied. The tool decodes twice, planting a label at every branch target and
emitting branches as text for the card's assembler to resolve.

## Bit-exactness, tested against real FMA3 hardware

The reference cannot come from the card, or it would be comparing the card
against itself. It comes from `fmaf()` on the host, which glibc lowers onto
the 5800X's FMA3 unit: real fused multiply-add hardware, single-rounded,
which is exactly what an AVX-512 `vfmadd` would have produced.

The multiply-add kernel, run at four different byte alignments so the
unaligned access pair is exercised both straddling a cache line and not:

```
offset  0: all 4096 lanes bit-identical
offset  4: all 4096 lanes bit-identical
offset 32: all 4096 lanes bit-identical
offset 60: all 4096 lanes bit-identical
```

The inputs include denormals, both infinities, a signalling NaN, both
signed zeros and the largest finite float. The polynomial kernel, 30
chained multiply-adds per element, is bit-identical over 65536 lanes at
every thread count from 1 to 228.

Three independent paths agree to the bit: the host's scalar FMA3, the
host's AVX2, and the card's translated MVEX.

## The card as one 912-lane engine

A Knights Corner vector unit is 512 bits wide, which is exactly the width of
an AVX-512 instruction: 16 float32 lanes on both machines. One VPU covers
one whole AVX-512 instruction with nothing left over, so a single
instruction cannot usefully be split across VPUs; doing so would hand each
one a fraction of a lane and pay ring synchronisation for the privilege.

What makes the 57 VPUs act as one engine is splitting the data the
instruction stream runs over. Each VPU runs the entire translated sequence
on a disjoint slice, and the card presents **912 float32 lanes** behind one
AVX-512 interface. Slices are cut on whole vectors, which are exactly
64-byte cache lines, so no two cores ever contend for a line.

| threads | GFLOP/s | scaling |
| --- | --- | --- |
| 1 | 7.33 | 1.0x |
| 16 | 117.05 | 16.0x |
| 32 | 206.14 | 28.1x |
| **57** | **313.52** | **42.8x** |
| 114 | 306.07 | 41.8x |
| 228 | 183.32 | 25.0x |

## Against the host, each machine using the widest unit it has

The host kernel had to be written in AVX2 intrinsics. The plain C loop
compiles to scalar `vfmadd213ss` here, since gcc would not vectorise across
elements, and measuring vectorised card code against a scalar host loop
would have produced a 3.17x "win" that meant nothing.

| | GFLOP/s | runs AVX-512 |
| --- | --- | --- |
| host AVX2, 16 threads, cache resident | 426.13 | no |
| card translated AVX-512, 57 threads, cache resident | **412.92** | yes |
| host AVX2, 16 threads, from DRAM | 326.18 | no |
| card translated AVX-512, 57 threads, from DRAM | **313.52** | yes |

## Four wrong guesses, corrected by measurement

**The unaligned pair would cost 2x.** It costs 1.07x to 1.09x. On aligned
data the high half addresses the line 64 bytes up, which the next iteration
reads anyway, so the second instruction behaves like a prefetch. The
translator does not need to prove alignment to perform well.

**Thread placement was limiting the scaling past 57 threads.** Pinning
core-major, using the card's real topology (CPU 0 is core 56 thread 3, and
CPUs `1+4k` through `4+4k` are core `k`), made it slightly *worse*:
209.79 against 230.84 GFLOP/s.

**The coefficient loads were limiting it.** Every multiply-add took its
coefficient from memory, 32 L1 reads per iteration instead of 2. Hoisting
all 30 into registers, which also exercised `zmm16` to `zmm31` and their
encoding extension bits, gained 2 percent.

**It was the dependent chain**, which was right. A single Horner chain is a
dependency graph one node wide and reached 14.6 percent of a core's peak.
Four independent chains took 57 threads from 235.50 to 308.86 GFLOP/s.

## Two measurement traps

The first sweep created and joined its threads inside the timed region. At
228 threads on a 1.1 GHz in-order core that is roughly 0.58 ms per thread,
so the 228-thread number was mostly `clone()`. Raising the work per thread
creation twentyfold changed it from 9.53 to 185.53 GFLOP/s and turned a
curve that collapsed after 16 threads into one that scales to 57.

The first kernel tried was `d[i] = a[i]*b[i] + c[i]`: 2 flops per 16 bytes
touched. It measured the memory system at 28.5 GB/s and never exercised the
vector unit. Arithmetic intensity is what decides whether a translated
kernel is worth sending to the card at all.

## What is not done

- Divide and square root are refused rather than synthesised. They need
  Newton-Raphson from `vrcp23ps` plus an FMA residual correction, and
  float64 has no reciprocal primitive at all, so its seed has to come down
  through `vcvtpd2ps`.
- `vmaxps` and `vminps` are refused. The compare-and-blend expansion is two
  instructions and is described in the research note, but substituting the
  card's instruction directly would be wrong only on NaN inputs, which is
  the kind of bug that survives testing.
- Zeroing masking, embedded rounding and embedded broadcast are refused.
  All three are expressible on the card; none is wired through the encoder.
- Gather and scatter need a loop, because the card completes only a subset
  of elements per issue. That is control flow inside what was one AVX-512
  instruction, and the region model has to carry it.
- The translator handles one function per file, and memory operands must be
  `[base + displacement]` because the MVEX encoder emits no SIB byte.
- Nothing dispatches automatically yet. The host does not trap `#UD` and
  route a faulting region to the card; kernels are translated ahead of time
  and run there deliberately.

Every one of these is refused by name with a reason rather than
approximated, which is the rule the translator is built on: a wrong answer
that looks like a right answer is the only outcome that would make the
exercise worthless.
