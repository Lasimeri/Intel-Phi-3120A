# membw.c

How much of the card's GDDR5 bandwidth a thread army can actually reach.

```
cc -O2 -o membw membw.c -lpthread
./membw [MiB_TOTAL] [THREADS] [REPS]
```

Each thread owns a disjoint slice, so there is no sharing and no false
sharing; four independent accumulators keep four loads in flight per
thread, because an in-order core with a dependent chain would measure
latency rather than bandwidth. Loads are scalar 8-byte integers: no VPU,
and KNC has no prefetch instructions at all (they are in the deletion
list, ISA reference 327364-001 appendix B.2), so this is the floor that
ordinary C reaches without hand-written vector code.

## Measured 2026-09-19

| Machine | Threads | Read bandwidth |
| --- | --- | --- |
| Card, 1 thread per core | 57 | 54.4 GB/s |
| Card, 2 threads per core | 114 | **80.5 GB/s** |
| Card, 3 threads per core | 171 | 74.3 GB/s |
| Card, 4 threads per core | 228 | 66.0 GB/s |
| Host (Ryzen 7 5800X, dual-channel DDR4) | 16 | 35.7 GB/s |

4 GiB working set, 6 repetitions, identical source on both machines.

Two readings, and both matter more than the headline number:

- **The card has 2.25x the host's read bandwidth**, and this is the one
  axis on which it wins without hand-written MVEX. Scalar compute is 0.14x
  the host (`2026-09-15-pi-bbp.md`) and scalar floating point 0.25x
  (`2026-09-15-mandelbrot.md`); only the VPU beats the host on compute, at
  1.47x (`2026-09-15-vpu.md`), and only through generated `.byte`
  sequences. Memory-bound work is therefore the card's natural fit and
  needs no special toolchain.
- **Two threads per core is the peak, not four.** Beyond that the four
  threads of a core contend for the same L2 port and the curve turns over.
  Anything tuned for this card should size its pool at 114, not 228, unless
  it is latency-bound rather than bandwidth-bound.

80.5 GB/s is 34% of the 240 GB/s Intel ARK quotes for the part. The gap is
what scalar loads cost: no vector loads to fetch 64 bytes per instruction,
and no prefetch to run ahead. A VPU load path would close part of it, which
is the obvious next measurement.
