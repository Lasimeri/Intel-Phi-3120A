# main.rs (knc-demo)

The card-side Rust demo: proves the binding works and measures it, so the
Rust number sits beside the C and C++ ones from the same machine.

```sh
/tmp/knc-demo [threads] [blocks-per-thread] [reps]
```

Two parts.

**Correctness, every width.** For each of 1 to 32 it packs a block, unpacks
it, and also unpacks the same packed bytes with a scalar model of the
layout written in this file. Both have to return the original values, so a
kernel that agreed with its opposite number on a wrong layout would still
be caught. Then it checks that `Codec::new(0)` and `Codec::new(33)` are
errors, which is the one behaviour the crate adds that the C library does
not have.

**Throughput,** at five widths across the thread count given. The buffers
are sized so the working set leaves cache behind; the thread pool is
created inside the timed region, so a low `reps` at a high thread count
measures thread creation instead (`card/lib/knc/knc_bench.cpp` prints that
cost; this one does not, so use `reps` of at least 256 at 228 threads).

Results in `src/lib.md` and `docs/results/2026-09-20-libknc.md`.
