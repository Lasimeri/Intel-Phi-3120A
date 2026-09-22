# 2026-09-22: the transport, taken apart

The morning's record said the co-processor was bound by its transport
"by more than an order of magnitude": 65536 elements took 2.9 ms of
which 0.08 ms was compute, and 16 M elements 70 ms of which 3.2 ms.
This is what that time was, and what is left. All numbers are card 0
(3120A, Gen2 x8) unless a row says card 1 (Gen2 x4, on the chipset);
`phi -c N vpu poly --n N --repeat 6`, the last runs of each, and
`card/examples/blkbench.c` for the raw block path.

| elements | before (morning) | after | of which compute |
| --- | --- | --- | --- |
| 65536 | 2.9 ms | **0.50 ms** | 0.05 ms |
| 1048576 | 17.2 ms | **3.0 to 3.5 ms** | 0.2 ms |
| 16777216 | 69.8 ms | **44 ms** | 2.9 ms |
| 65536, card 1 | 3.7 ms | 0.71 ms | 0.06 ms |
| 16777216, card 1 | 117 ms | 87.5 ms | 2.9 ms |

Against the only alternative on a host with no AVX-512, software
emulation of the same 65536-element kernel at 26.5 ms
(`2026-09-22-avx512-coprocessor.md`), the card is now 53x, not 9x.
64 MiB each way moves at the link: 20.4 ms in and 20.2 ms out on x8
(3.3 GB/s), 42 ms each way on x4.

## What the time was

Four things, found in this order, each measured before the next.

### 1. The host served one DMA copy at a time

`phictl`'s block service popped one record, submitted one descriptor,
spun on its status word, pushed the completion, and went back for the
next. `DmaChannel` had only a synchronous `copy`. The self-test, which
is that loop over 512 KiB copies, reported 1.4 GB/s.

`phi-hw/src/dma.rs` now has `submit` (one descriptor line, returns the
sequence number, up to 63 in the ring), `poll` (the larger of the two
status words is the last copy done; the engine runs the ring in order)
and `wait`; the service submits every host-memory read or write as its
record arrives and answers records as their copies complete, draining
before anything served in order (identify, flush, an image file). The
card side already allowed 32 requests in flight; nothing there changed.

Card 0, 16 M elements, before and after, the old worker:

| | pull 64 MiB | push 64 MiB |
| --- | --- | --- |
| one copy at a time | 43 ms | 23 ms |
| pipelined | 24.7 ms | 19.2 ms |

Card 1 went from 61 and 54 ms to 46 and 41 ms, which is its x4 link.
The self-test still reports the serial number (it is a serial loop).

### 2. A request was not one record; it was one per scattered page

That was not the 10x the note expected, so the records were counted.
Two counters were added to the daemon's traffic report (`phictl
traffic`: bytes and copies per direction, so the mean copy size), and
the block path timed from the card at fixed total and varying request
size:

| `pread`, 4 KiB pages | per call | records per call |
| --- | --- | --- |
| 4 KiB | 89 us | 1 |
| 64 KiB | 405 us | 15 |
| 512 KiB | 1.8 ms | 88 |
| 16 MiB | 7.4 to 10 ms | 362 |

In direct mode the card's driver posts one record per physically
contiguous run of the request's pages (`blk_rq_map_sg`), and a buffer
from `posix_memalign` is scattered 4 KiB pages where it is not lucky.
Each record cost the host about 20 us serialised: the request ring lives
in card memory, and reading a record is four 8-byte aperture reads plus
the indices, each a PCIe round trip, before the descriptor is written.
The pipeline hid the DMA time but not that.

The card kernel has `hugetlbfs` (no transparent huge pages), so the
worker's buffers now come from 2 MiB pages (`mmap(MAP_HUGETLB)`, with a
4 KiB fall-back; `scripts/phi-vpu.sh start` reserves 512 pages) and a
512 KiB request is one record:

| `pread`, 2 MiB pages | per call | rate |
| --- | --- | --- |
| 4 KiB | 89 us | |
| 64 KiB | 108 us | |
| 512 KiB | 244 us | 2.1 GB/s |
| 4 MiB | 1.32 ms | 3.2 GB/s |
| 16 MiB | 5.2 ms | 3.2 GB/s |

Writes are the same within noise. The 4 KiB row is the floor: one
record's round trip through the card's block layer, the ring, the host
loop and the engine, about 85 us.

### 3. Both sides slept between requests

With records no longer the cost, the 65536-element request still took
1.6 to 2.5 ms wall, the best runs 0.5 ms: the worker's two `pread`s and
one `pwrite` each arrived after a gap, and both pollers had gone to
sleep. The host service spun 3 ms after its last record and then slept
200 us between polls; the card's completion poller spun 2000 iterations
and then napped 200 to 400 us. The host now polls for 20 ms after
activity (`SPIN_AFTER_ACTIVITY`). The card is kernel patch 0029, and it
took three tries.

- **Spin 10 ms after every completion.** Small requests dropped to
  0.48 ms wall. The 16 M compute went from 3.2 ms to 8.7 ms: the poller
  is a kernel thread, `wake_up_process` from the posting path put it on
  the poster's CPU (the worker's dispatcher, CPU 0), and the two split
  that CPU through the compute phase.
- **Spin only while requests are outstanding, plus 200 us.** Compute
  back to 2.6 ms (better than the original 3.2: the original poller's
  2000-iteration spin had been landing on the same CPU too). Small
  requests back up to 1.1 to 1.5 ms: the poller was napping when the
  next request came, and the wake path costs about 0.3 ms on this
  kernel.
- **Pin the poller and spin 5 ms.** Pinned one core below the last (the
  last core's fourth thread is CPU 0; the worker uses one thread per
  core, so the upper threads are free). Small requests 0.48 ms; compute
  5.9 ms. A spinning sibling thread halves the VPU thread on the same
  core: Knights Corner issues from one thread per clock, and a thread
  that always has an instruction ready takes every other slot.

The way out is the instruction Knights Corner has for this: `DELAY r32`
(ISA reference 327364-001, appendix A, "Stall Thread"), which stops the
thread's fetch and issue for a count of clocks and gives the core to
its siblings. MPSS's own kernel used it for `cpu_relax`. Neither our
assembler nor objtool know it, so it is four raw bytes in
`arch/x86/kernel/knc_delay.S` (`VEX.128.F3.0F.W0 AE /6`, `c5 fa ae f7`
for `edi`), and the x86 opcode map gets a `(v1)` on the slot so the
kernel's decoder, which objtool uses, can size it; objtool otherwise
fails the link with "can't decode instruction". The poller now stalls
3300 clocks (3 us) between polls while requests are outstanding:

| poller between polls | 65536 elements | 16 M compute |
| --- | --- | --- |
| spinning | 0.48 ms | 5.9 ms |
| `delay 550` | 0.49 ms | 3.8 ms |
| `delay 3300` | 0.50 ms | 2.9 ms |
| napping (reference) | 1.1 to 1.5 ms | 2.6 ms |

### 4. What is left

- The 85 us floor per record: card syscall and block layer, a ring
  write, the host's aperture reads of the record, the descriptor, the
  completion write, the card poller. A request ring in host memory (the
  card writes it with posted PCIe writes, the host reads its own memory)
  would take the host's aperture reads out of it; MPSS laid its rings
  out that way. Not done: with 512 KiB records the pipeline hides the
  per-record cost, and the floor only matters below about 64 KiB.
- The worker pulls its coefficients in a separate `pread` (one more
  round trip per request). They could ride in the control window.
- A call after a 1 ms idle gap costs about 85 us more than one
  back-to-back, on both cards, with both pollers awake; not chased.
- Card 1's compute at 16 M is the same as card 0's now that neither
  poller lands on a worker CPU; the morning's 374 against 316 GFLOP/s
  was where the poller happened to run.

## Machine state

Both cards on kernel build #45 (patches 0001 to 0029), swap off on
`/dev/phiblk1`, a huge-page worker on each (`phi -c N vpu status`),
`phi traffic` reporting copies. `card/examples/blkbench.c` stays as the
transport's timer: `phi -c N put`, `cc` on the card, `/tmp/blkbench
/dev/phiblk1 16 0 huge`.
