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
on 228 threads (1.05 Giter/s; fewer deep pixels). In this first version
writing the PNG was single-threaded deflate: 0.94 s on the card for the
full set, 1.6 s for the busier detail, against 0.05 s on the host.

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

## 8K (7680x4320), maxiter 2000, full set, both stages on all threads

At 8K the raw image is 99.5 MB and the single-stream `compress2` of the
first version would have dominated the run, so the second version of
`mandel.c` parallelises the PNG stage too (128 KiB pieces, independent raw
deflate states primed with the preceding 32 KiB, `Z_SYNC_FLUSH` between
pieces, `crc32_combine` and `adler32_combine` for the checksums; the
method pigz uses) and has the render threads write straight into the
scanline buffer. Compiled on the card in 1.44 s.

| where | threads | render | throughput | deflate | write | whole run |
| --- | --- | --- | --- | --- | --- | --- |
| card | 228 | 8.855 s | 1.66 Giter/s, 3.75 Mpix/s | 0.369 s | 0.040 s | 9.26 s |
| card | 57 | 14.783 s | 0.995 Giter/s | 0.297 s | 0.039 s | 15.1 s |
| card, first version (single-stream PNG) | 228 | 8.859 s | 1.66 Giter/s | 13.816 s (png) | | 22.7 s |
| host, native SSE2 | 16 | 2.304 s | 6.39 Giter/s | 0.032 s | 0.004 s | 2.34 s |

The deflate stage handles 760 pieces, 99.5 MB to 7.6 MB. Notes:

- The PNG stage went from 13.8 s to 0.37 s (37 times) on 228 threads; on
  57 threads it is faster still (0.30 s). The likely reason: deflate is
  bound by cache and memory traffic rather than by pipeline latency, so
  the three extra hardware threads per core add contention and nothing
  else, and 760 pieces over 228 threads leaves a ragged tail. The 1080p
  stage went from 0.94 s to 0.21 s (48 pieces, so at most 48 threads busy).
- The render rate at 8K is a little higher than at 1080p (1.66 against
  1.48 to 1.52 Giter/s): the same view has the same per-pixel work, but
  thread start-up and the end-of-run tail are amortised over 16 times more
  rows.
- Piece boundaries cost 0.2 percent at 1080p (651,744 to 653,227 bytes)
  and 0.5 percent at 8K (7,528,025 to 7,565,991 bytes) against the single
  stream.
- The card's image differs from the host's in 2,413 of 33,177,600 pixels
  (`magick compare -metric AE`, PSNR 43 dB): boundary pixels whose escape
  iteration moves by one between x87 extended-precision intermediates and
  SSE2 doubles.
- Fetching the 7.6 MB PNG through the control socket took 1.05 s
  (7.2 MB/s).
- The whole 8K run on the card (9.26 s) is 0.25 of the host's 16 threads
  (2.34 s), the same ratio as at 1080p.

The 8K image is not in the repository (7.6 MB); it is reproduced by the
command in `card/examples/mandel.md` and was saved on the host as
`~/phi-mandelbrot-8k.png`.
