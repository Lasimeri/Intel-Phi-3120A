# avx512_poly: all 57 vector units on one AVX-512 instruction stream

A degree-30 polynomial evaluated by Horner's method in float32, which is
the shape of a vectorised transcendental function: 30 fused multiply-adds
per element against one load and one store.

```
host/target/debug/avx512-xlate card/examples/avx512_poly.avx512.s \
    --name poly_kernel_x8 --out card/examples/avx512_poly.S
cc -O2 -o avx512_poly avx512_poly.c avx512_poly.S -lpthread   # on the card
./avx512_poly 2097152 200
```

264 EVEX instructions translate, 7 scalar pass through. The harness checks
the card's output against the host's FMA3 reference before it times
anything, because a fast wrong answer is worth nothing.

## How the card is made to act as one unit

A Knights Corner vector unit is 512 bits wide, which is exactly the width of
an AVX-512 instruction: 16 float32 lanes on both machines. One VPU
therefore covers one whole AVX-512 instruction with nothing left over, and
splitting a single instruction across several VPUs would hand each a
fraction of a lane while paying ring-interconnect synchronisation to do it.

What makes the 57 VPUs act as one engine is splitting the **data** the
instruction stream runs over. Every VPU executes the entire translated
sequence on a disjoint slice, so the card presents 57 x 16 = **912 float32
lanes** behind one AVX-512 interface.

Slices are cut on whole vectors, and a 16-float vector is exactly one
64-byte cache line, so no two cores ever hold the same line. A slice
boundary inside a line would put two cores into a coherence fight over data
neither of them shares.

## Why the kernel is unrolled eight ways

Horner is a dependency graph one node wide: every multiply-add waits on the
one before it. An in-order core with a multi-cycle vector unit issues one
instruction every few cycles no matter how much work is left, so a single
chain reached only 14.6 percent of a core's peak. Eight independent chains
give the issue logic something ready at all times.

| chains per thread | 1 thread | 57 threads |
| --- | --- | --- |
| 1 | 5.14 GFLOP/s | 235.50 GFLOP/s |
| 4 | 7.54 | 308.86 |
| 8 | 7.33 | **313.52** |

## Measured 2026-09-21

Bit-identical to the host at every thread count. Throughput, 2097152
elements, eight chains:

| threads | GFLOP/s | scaling |
| --- | --- | --- |
| 1 | 7.33 | 1.0x |
| 8 | 60.81 | 8.3x |
| 16 | 117.05 | 16.0x |
| 32 | 206.14 | 28.1x |
| **57** | **313.52** | **42.8x** |
| 114 | 306.07 | 41.8x |
| 228 | 183.32 | 25.0x |

Scaling is close to linear to one thread per core and then stops. Past 57
the extra contexts do not help: the kernel is already issuing enough
independent work per thread that there is nothing left for a second context
on the same core to hide, and the memory system is carrying 41.8 GB/s.

## Against the host

The 5800X has AVX2 and FMA3 and no AVX-512 whatsoever. The comparison is
the same computation, each machine using the widest vector unit it has,
with the host kernel written in AVX2 intrinsics (a plain C loop compiles to
scalar `vfmadd213ss` here, which would not have been a fair opponent).

| | GFLOP/s | runs AVX-512 |
| --- | --- | --- |
| host AVX2, 16 threads, cache resident | 426.13 | no |
| card translated AVX-512, 57 threads, cache resident | 412.92 | yes |
| host AVX2, 16 threads, from DRAM | 326.18 | no |
| card translated AVX-512, 57 threads, from DRAM | 313.52 | yes |

**0.97x of the host, on an instruction set the host cannot execute at all.**

## Two measurement traps hit while producing these numbers

The first sweep created and joined its threads inside the timed region.
At 228 threads on a 1.1 GHz in-order core that is roughly 0.58 ms per
thread, so the 228-thread figure was mostly `clone()`. Raising the work per
thread creation twentyfold changed 228 threads from 9.53 GFLOP/s to
185.53 and turned a curve that fell after 16 threads into one that scales
to 57.

The first kernel tried was `d[i] = a[i]*b[i] + c[i]`, which is 2 flops per
16 bytes touched. It measured the memory system at 28.5 GB/s and never
exercised the vector unit at all. Arithmetic intensity is what decides
whether a translated kernel is worth sending to the card.
