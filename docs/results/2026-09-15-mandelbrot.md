# 2026-09-15: Mandelbrot on the card, a CPU benchmark

`card/examples/mandel.c` (double precision escape-time iteration, rows
handed out dynamically to N threads, smooth colouring, PNG through zlib)
was pushed as source and compiled on the card by its own clang
(`cc -O2 -o mandel mandel.c -lz -lpthread`, 1.17 s). The card had been
booted unprivileged (`scripts/phi-up.sh --ssh`).

## Render times, 1920x1080, maxiter 2000, full set

| where | threads | render | throughput |
| --- | --- | --- | --- |
| card, 1.1 GHz in-order, x87 | 228 | 0.620 s | 1.48 Giter/s, 3.35 Mpix/s |
| card | 57 (one per core) | 0.943 s | 0.98 Giter/s |
| card | 1 | 54.19 s | 0.017 Giter/s |
| host Ryzen 7 5800X, the card binary (x87 code) | 16 | 0.360 s | 2.56 Giter/s |
| host, native clang -O2 (SSE2) | 16 | 0.146 s | 6.29 Giter/s |
| host, native | 1 | 2.010 s | 0.46 Giter/s |

A detail (centre -0.7453+0.1127i, width 0.006, maxiter 4000) took 0.255 s
on 228 threads (1.05 Giter/s; fewer deep pixels). Writing the PNG is
single-threaded deflate: 0.94 s on the card for the full set, 1.6 s for
the busier detail, against 0.05 s on the host.

## Reading

- Scaling on the card: 228 threads are 87 times one thread; the four
  threads of a core add 1.5 times over one thread per core (in-order
  cores hide little latency, so SMT is worth less than on the host).
- Per thread the card is 27 times slower than one Zen 3 core at 4.85 GHz
  with SSE2 (0.017 against 0.46 Giter/s): 4.4 times the clock, out-of-order
  execution, and SSE2 against x87 (the same source in x87 form on the host
  runs at 0.41 of the native speed, 2.56 against 6.29 Giter/s on 16
  threads).
- The whole card, all 228 threads, delivers about a quarter of the host's
  16 threads on this kernel: the equivalent of roughly three desktop
  cores, for 300 W. The vector unit that would change that picture (16
  doubles per instruction, 512-bit) is the one part of the chip this port
  does not use: the compiler emits x87 only.

Image: `images/2026-09-15-mandelbrot-1920x1080.png` (the 228-thread render,
fetched with `phictl get`).
