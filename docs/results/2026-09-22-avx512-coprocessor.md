# The card as an AVX-512 co-processor, end to end

2026-09-22. Host: Ryzen 7 5800X (AVX2 and FMA3, no AVX-512), Linux
7.2.6-1-cachyos. Card: Xeon Phi 3120A, 57 cores, 228 hardware threads.
Follows `2026-09-21-avx512-translation.md`, which established that a
translated AVX-512 kernel runs on the card bit-identically. This record
is about the card doing that **on the host's behalf**: the host writes
data into shared memory, rings a doorbell, the card's vector units run
the kernel, the result comes back, and every lane is checked against the
host's own FMA3 hardware.

```
scripts/phi-vpu.sh deploy && scripts/phi-vpu.sh start 57
host/target/release/phi-vpu poly --n 16777216 --threads 57 --repeat 4
```

## What was wrong and what it cost

The first worker created and joined its threads inside the request
handler. Thread creation costs about 0.58 ms on this card, so compute
time grew with the thread count and 57 threads spent 33 ms creating
themselves to do 0.3 ms of work:

| threads | compute, 65536 elements | worker |
| --- | --- | --- |
| 1 | 1.854 ms | threads per request |
| 8 | 5.327 ms | |
| 57 | 34.865 ms | |
| 57 | **0.077 ms** | persistent pool, warm |

The pool is created once, each thread pinned core-major to its own
hardware thread, and a request is one generation: the dispatcher writes
the slices, bumps a word, runs the last slice itself, and waits for a
counter. A pool thread spins on the word for 200 ms after its last job,
then parks in a futex. Both halves are measured below because they cost
different things.

## Compute, all lanes bit-identical to the host's FMA3

57 threads, one request each, pool warm (a request within 200 ms of the
last), best of four:

| elements | compute | GFLOP/s on the vector units |
| --- | --- | --- |
| 65536 | 0.081 ms | 48 |
| 1048576 | 0.313 ms | 201 |
| 4194304 | 0.716 ms | 351 |
| 16777216 | 3.185 ms | **316** |

316 GFLOP/s at 16 million elements is the figure the standalone
benchmark reached on the card the day before (313.5 to 316.9), so
nothing is lost to the offload path once the request is large enough to
amortise the dispatch. The 4-million point is higher because each
thread's slice (294 KiB in, 294 KiB out) partly fits its 512 KiB L2;
an earlier run of the same size gave 1.05 ms, so the spread at that size
is real. Below a million elements the fixed dispatch cost (about 60 us:
a cache-line transfer to 56 spinning cores, 56 locked increments back)
is visible.

Thread sweep at 1048576 elements, warm:

| threads | compute | GFLOP/s |
| --- | --- | --- |
| 1 | 8.867 ms | 7.1 |
| 8 | 1.477 ms | 42.6 |
| 32 | 0.693 ms | 90.8 |
| 57 | 0.283 ms | 222 |
| 114 | 0.273 ms | 230 |
| 228 | 0.556 ms | 113 |

## Warm against parked

With the spin window at 20 ms the host's own work between requests
(refilling and re-checking 4 MiB) was enough to park the pool, and every
request paid to wake it. Same request, 1048576 elements, 57 threads:

| pool state at the doorbell | compute, six consecutive requests |
| --- | --- |
| spinning | 0.289, 0.319, 0.316, 0.303, 0.297, 0.284 ms |
| parked, woken by futex | 0.573, 0.629, 0.624, 0.599, 0.780, 0.541 ms |

Waking 56 parked threads costs about 0.3 to 0.5 ms. The default window
is therefore 200 ms; while parked the card is 99 percent idle (`top` on
the card), which is the point of parking at all.

## The dispatcher was pegging a core

The doorbell poll ran flat out for ever: CPU 0 at 100 percent in phitop,
one PCIe read of the request word per iteration. It now spins for the
200 ms window after the last request and then sleeps between polls.
`nanosleep` on the card kernel costs about 60 us over the requested
time (10 us asks for 72, 100 for 162, 500 for 563), so the interval sets
the idle doorbell latency:

| idle poll | CPU 0 busy, idle | first doorbell after 1 s quiet |
| --- | --- | --- |
| 100 us | 23.5% | 79 to 157 us |
| **500 us (default)** | **0.8%** | 70 to 681 us |
| 1000 us | 3.3% | 259 to 775 us |
| 2000 us | 2.7% | 983 to 1857 us |

Warm doorbell by the same measure (host wall minus the card's own total):
21 us. The whole card at 228 CPUs is under 0.1 percent busy with the
worker up and nothing to do.

Found while measuring this: a `pkill` in the same ssh command line as
`./phi-vpu-worker` matches its own shell, so nothing started, and the
host driver accepted the dead worker's readiness word and waited a
minute per request. The driver now clears the word and waits for a live
worker to re-assert it (five seconds, then a message).

## Where a request's time goes now

The same runs, whole request, 57 threads:

| elements | pull (DMA in) | compute | push (DMA out) | total |
| --- | --- | --- | --- | --- |
| 65536 | 2.3 ms | 0.08 ms | 0.55 ms | 2.9 ms |
| 1048576 | 15.2 ms | 0.31 ms | 1.7 ms | 17.2 ms |
| 4194304 | 24.7 ms | 0.72 ms | 6.2 ms | 31.6 ms |
| 16777216 | 43.4 ms | 3.2 ms | 23.2 ms | 69.8 ms |

The transport is the bound, by more than an order of magnitude, and it
varies between runs in a way the compute does not: the same 1048576
element pull measured 8.9 ms in one series and 15.2 ms in another, the
push 4.2 ms and 1.7 ms, and one 65536-element pull took 25 ms. Bulk data
goes through `/dev/phiblk1`, the DMA block path, because the
`/dev/phihost` mapping is uncached and streams at 50 MB/s. That path
serves one 512 KiB record at a time, each a round trip to the host
daemon with about 500 us of fixed latency (`2026-09-16-dma.md`), which
is what these numbers show: 16 MiB is 32 records, about 1.3 ms each on
the way in. Pipelining records in the host daemon and the card's block
driver (kernel patch 0026) is the next lever, and it is a transport
change, not a co-processor one.

## Against the only alternative

A host with no AVX-512 has exactly one other way to run this code:
in software. `phi512` performs an AVX-512 instruction in about 152 ns
once its site has been rewritten. The same 65536-element kernel, the
same AVX-512 machine code:

| | wall, whole program |
| --- | --- |
| software emulation on the host (`phi512`, sites rewritten) | 26.5 ms |
| the card, transport included | **2.9 to 4.0 ms** |

6.6x to 9x at the smallest size measured, and the ratio grows with size
because emulation scales with the instruction count while the card's
transport cost per element falls with larger records. Comparing the card
against the host's **native AVX2** instead (0.97x, in the previous
record) answers a different question, and not the one a host without
AVX-512 is asking.

## Verification, and the trap in the transport

Every run compares every output lane against `f32::mul_add` on the host
(`fmaf`, the host's FMA3 unit) and reports a speed only when all match.
All 16777216 lanes matched in every run above.

Three things had to be found before the numbers could be trusted:

1. **The card's page cache serves stale `/dev/phiblk1` data**, because
   the host changes that memory behind its back. The worker opens the
   device `O_DIRECT`, which then requires 4096-byte alignment of every
   offset and length; an unaligned output offset fails with status -3
   rather than corrupting anything.
2. **The worker owns the sequence reset.** Taking its baseline from the
   window made a request left by a previous run invisible for ever.
3. **A readiness flag written once races the host's clear of the
   window.** The card re-asserts it while idle.

## Machine state

`/dev/phiblk1` is the card's swap device at boot and is also the offload
window, so `scripts/phi-vpu.sh start` refuses while swap is on it. Swap
was taken off for this work (`swapoff /dev/phiblk1` on the card) and
`swapon` puts it back.
