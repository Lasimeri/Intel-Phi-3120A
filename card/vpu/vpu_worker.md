# phi-vpu-worker: the card side of the co-processor

Resident on the card. Polls a doorbell in host memory, and when a request
arrives runs an AVX-512 kernel, translated to the card's own instruction
set ahead of time by `host/crates/avx512-xlate`, across the vector units,
and writes the result back where the host can read it.

```
phi-vpu-worker [-v] [-s MS] [-i US] [threads]
```

`threads` is the most the worker will spread one request across (1 to
228, default 57). `-v` logs one line per request. `-s` is the spin window
and `-i` the idle poll interval, both described below.
`scripts/phi-vpu.sh` deploys, builds, starts and stops it from the host
(`PHI_VPU_ARGS` passes these options through `start`).

## The pool, and why it exists

Creating a thread on this card costs about 0.58 ms (a 1.1 GHz in-order
core, measured 2026-09-21 and again here). The first version of this
worker created and joined its threads inside the request handler, and
the result was a compute time that **grew** with the thread count:

| threads | compute per request, 65536 elements | measured |
| --- | --- | --- |
| 1 | 1.854 ms | 2026-09-22, per-request threads |
| 8 | 5.327 ms | |
| 57 | 34.865 ms | |
| 57 | **0.088 ms** | 2026-09-22, persistent pool, warm |

The pool is created once at start-up. Pool thread `t` is pinned to
`knc_cpu(t % 57, t / 57)`: core-major, so one thread lands on every core
before any core gets a second, which is what the two-cycle decoder wants
(SSDG 328207-002 section 2.1.2). The dispatcher, the thread that polls the
doorbell, is pinned to CPU 0, which is core 56's last hardware thread, so
the pool never shares its core until all 227 other hardware threads are
taken.

Each request is one generation. The dispatcher writes every thread's job
(a slice of whole 128-element chunks), bumps the generation word, runs
the last slice itself rather than spinning on a core that has work, and
waits for the done counter to reach the pool size. Every pool thread
acknowledges every generation, even one that gave it nothing, so there is
no per-request bookkeeping of who is expected.

## Waiting without a syscall, and parking without a busy card

A pool thread spins on the generation word for `-s` milliseconds after
its last job (default 20), then parks in a futex. The spin is what makes
a request that follows another cost nothing but a cache-line transfer;
the park is what keeps 56 cores from burning power while the host is
doing something else. Measured 2026-09-22, 1048576 elements, 57 threads:

| pool state when the doorbell rang | compute |
| --- | --- |
| spinning (within the window) | 0.302 ms |
| parked (idle 3 s) | 0.788 ms |

Waking 56 parked threads costs about 0.5 ms. The dispatcher issues the
wake only when the parked counter says someone is asleep, so the warm
path makes no system call at all.

## The dispatcher's own idle

The doorbell poll is one PCIe read per iteration, and the first version
did it flat out for ever: one hardware thread (CPU 0) at 100 percent,
reading host memory a million times a second to learn nothing, visible
in phitop as a pegged core. Now it spins only for the same `-s` window
after the last request and then sleeps `-i` microseconds between polls
(default 500). `nanosleep` on this kernel costs about 60 us over what is
asked (10 us asks for 72, 100 us for 162, 500 us for 563, measured
2026-09-22), which sets the idle doorbell latency. Measured on CPU 0
while idle, over 3 s, with the first doorbell after 1 s of quiet:

| `-i` | CPU 0 busy | idle doorbell (host wall minus card total) |
| --- | --- | --- |
| 100 us | 23.5% | 79 to 157 us |
| **500 us** | **0.8%** | 70 to 681 us |
| 1000 us | 3.3% | 259 to 775 us |
| 2000 us | 2.7% | 983 to 1857 us |

A warm doorbell is 21 us by the same measure. The cost of the default is
therefore up to 0.7 ms on the first request after 200 ms of silence,
against a transport that costs 3 ms for the smallest request, for a card
that is 99.9 percent idle when nothing is happening.

The card's musl ships no `linux/futex.h`; the syscall number (202) and
the two operation codes are defined in the source.

## Fences on a core that has none

Knights Corner has no `MFENCE`, `LFENCE` or `SFENCE` (ISA reference
327364-001, appendix B). x86 ordering makes a store visible before a
later store and a load before a later load without help, and every place
the worker relies on that has a compiler barrier and a comment. The one
place a **store must be visible before a following load** is the
generation bump followed by the read of the parked counter, and that is a
`lock addq $0, (%rsp)`, the same idiom libknc's vector store kernels use.
The done counter is a locked add, which is also a full fence, so a
thread's results are visible before its acknowledgement.

## Moving the data

Bulk data goes through `/dev/phiblk1`, the DMA path, not through the
`/dev/phihost` mapping, which is uncached and streams at 50 MB/s. Two
rules follow from opening the block device with `O_DIRECT`, which is
required because the host changes this memory behind the card's back and
the page cache would serve whatever the last reader saw:

- every offset and every length is a whole number of 4096-byte blocks,
  so the worker rounds lengths up and the host must lay regions out on
  block boundaries with nothing in the slack (`vpu_proto.h`)
- buffers persist across requests and only grow; the first version
  allocated and freed them per request, paying a page fault per 4 KiB
  on first touch

The transport is now what bounds a request. The host-memory block path
serves one 512 KiB record at a time with about 500 us of fixed latency
each (`docs/results/2026-09-16-dma.md`), so 4 MiB each way costs about
9 ms in and 4 ms out against 0.3 ms of compute. Pipelining records in
the host daemon and the card driver is the next lever, and it is not in
this file.

## Status codes

| value | meaning |
| --- | --- |
| 0 | done |
| -1 | could not reserve buffers |
| -2 | reading the input failed |
| -3 | writing the output failed (an unaligned offset is the usual cause) |
| -4 | `n` is zero or an offset is not block aligned |
| -5 | unknown kernel number |

## Things that cost time here

- **The worker owns the sequence reset.** Taking the baseline from
  whatever the window held made a request left by a previous run
  invisible for ever. Both counters are zeroed at start-up; the host
  starts from one.
- **The readiness word is re-asserted while idle**, or the host's clear
  of the window before its first request wipes the flag it is about to
  wait for.
- `pkill -f phi-vpu-worker` typed over ssh matches the ssh command line
  and kills the session. `scripts/phi-vpu.sh` uses a bracket class.
