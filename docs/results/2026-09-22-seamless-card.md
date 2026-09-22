# 2026-09-22: the card executes the program's AVX-512 itself

The correction of the day before: `scripts/phi512.sh` ran a program's
AVX-512 in a host-side interpreter (`2026-09-22-seamless-test.md`).
Now it does not. An ordinary AVX-512 program, unmodified, run through
the wrapper, has every one of its AVX-512 instructions executed by the
Xeon Phi's vector units, as MVEX, the card's own encoding of the same
operations. Nothing is interpreted anywhere on the path; the emulator
is reachable only by asking for it (`PHI512_EMULATE=1`), and without a
card the program stops with the reason.

## The run

`tools/avx512-seamless-test.c`, 65536 elements, card 0, commit 6f1b805:

```
phi512: AVX-512 will be executed by the Xeon Phi card's vector units
phi512: card ran 0x5614b7132431..0x5614b7132483 (15 instructions, 5 AVX-512) to 0x5614b7132483: 5 chunks, 66 written back, 6 faults; fetch 9531 us, run 16514 us, write back 9268 us, total 32303 us
phi512: card ran 0x5614b71324ae..0x5614b71324d4 (7 instructions, 3 AVX-512) to 0x5614b71324d4: 4 chunks, 1 written back, 4 faults; fetch 5913 us, run 9525 us, write back 4841 us, total 19289 us
phi512: card ran 0x5614b7132517..0x5614b7132543 (7 instructions, 3 AVX-512) to 0x5614b7132543: 3 chunks, 0 written back, 2 faults; fetch 3114 us, run 3374 us, write back 0 us, total 7229 us
phi512: card ran 0x5614b713254b..0x5614b7132597 (13 instructions, 7 AVX-512) to 0x5614b7132597: 4 chunks, 66 written back, 5 faults; fetch 6095 us, run 12441 us, write back 8399 us, total 24615 us
phi512: the card ran 4 region(s); nothing was emulated
polynomial (31 fmadd per vector):    32.745 ms  every lane bit-identical to the scalar reference
dot product (fmadd + reduce):        19.300 ms  bit-identical (-28.6031036)
integers (compare, mask, mullo):     31.858 ms  every lane identical
PASS: the AVX-512 code ran and its answers are right
```

Four regions: the polynomial's two nested loops (a broadcast, aligned
loads and stores, fused multiply-adds with an embedded broadcast from
memory); the dot product's loop and the extract that starts its reduce
(a thunk: a 128-bit permute and a masked zero); the integer setup (the
all-ones idiom as a thunk, three shifts by immediate) and the integer
loop (a compare into a mask, a masked multiply). The program then ran
its scalar checks natively and every lane matched.

## How

`docs/results/2026-09-22-seamless-test.md` listed what the join would
take; this is what it took. The SIGILL handler (`host/crates/phi512/src/
offload.md`) walks the code graph from the fault to a region bounded by
what the card cannot run, rewrites the EVEX instructions to MVEX in
place at the byte level (`avx512-xlate/src/rewrite.md`: the two share
their ModRM, SIB, displacement and disp8 scaling, so the prefix payload
is all that changes), writes `ud2` at every exit, and ships the 2 MiB
chunk of text, a thunk area and the register file. The card
(`card/vpu/vpu_exec.md`) maps the code at the program's own addresses,
pages the program's memory in from SIGSEGV through a mailbox the host
serves, runs, and sends back only the 64-byte lines the region changed.
Kernel patch 0030 keeps the vector unit across a signal handler, which
the card side needs and which fixes a latent defect for every card
program.

What went wrong on the way, each measured before the next: the
descriptor beyond the worker's control mapping (SIGBUS); a thunk
constant the card refused for its alignment (a vector operand faults
when misaligned); the thunk area landing inside the code chunk; a
whole-chunk write-back that overwrote the handler's own alternate stack
and heap next to the program's arrays (hence the line-granular diff);
the compiler emitting `vpcmpd` with a predicate where the disassembler
shows `vpcmpeqd`.

## What it costs, and why

| region | fetch | run | write back | total |
| --- | --- | --- | --- | --- |
| polynomial, 65536 elements | 9.5 ms (5 chunks) | 16.5 ms | 9.3 ms (66 pages) | 32 ms |
| dot product | 5.9 ms (4) | 9.5 ms | 4.8 ms (1 page) | 19 ms |

Against 22.5 ms for the polynomial in the emulator and 0.50 ms on the
explicit path. Three costs, each with a known fix, in order:

1. **A fresh huge page per chunk.** The card kernel zero-fills a 2 MiB
   page as the DMA lands: about 1 ms of the 1.9 ms a chunk costs. A pool
   of pre-faulted huge pages moved into place with `mremap` keeps the
   pages and their contents.
2. **Snapshots and diffs on one thread.** The snapshot at a chunk's first
   write (2 MiB `memcpy`) and the line diff at the exit run on one KNC
   thread at a few hundred MB/s; 56 pool threads sit idle meanwhile.
3. **The loop on one hardware thread.** Splitting a loop across the pool
   needs, from the analysis: the induction register and its step, the
   bound, one exit, no register carried across iterations except the
   induction, stores indexed by it and disjoint from loads (bases known,
   including those reloaded from a stack slot). The polynomial and
   integer loops qualify; the dot product (an accumulator across
   iterations) runs single-threaded and stays bit-identical. Each thread
   gets the same code with its own index range; the last thread's
   register file is the sequential one.

Also open: chunks fetched afresh per region (the text and the stack
chunk are the same every time), the code chunk resent per region, one
region at a time, other host threads unsynchronised.

## Evening: the three costs, fixed, and then the rest

Commit after 85a6f84. Card 0, the same test, every size bit-identical:

| elements | polynomial | dot product | integers | emulated polynomial |
| --- | --- | --- | --- | --- |
| 65536 | 7.7 ms | 3.3 ms | 5.5 ms | 22.5 ms |
| 1048576 | 16.2 ms | 10.7 ms | 10.6 ms | 376 ms |
| 16777216 | 98 ms | 139 ms | 91 ms | about 6 s |

What changed, each measured before the next:

1. A planner (`host/crates/phi512/src/plan.md`) resolves every address
   a phase touches from the register file, stack slots included, and
   finds the loop to split; the card fetches exactly those pages and
   writes exactly the stored ranges back (no snapshots, no diffs, no
   faults), and the loop runs on 57 threads, the last of them keeping
   the original bound so the final registers and flags are the
   sequential ones. The dot product's accumulator keeps it on one
   thread, bit-identical. A miss falls back to demand mode for that
   phase, after the card refuses without writing back.
2. Regions run in phases (prologue, loop, epilogue), each dispatched
   with the exact registers the previous returned.
3. On the card: a pool of 256 pre-faulted huge pages moved into place
   with `mremap`; chunks kept mapped across regions (every map change
   flushed 57 TLBs; a region's unmap was 2 ms); the descriptor, thunk
   and code pages as one bundle in one DMA (the uncached read was
   0.4 ms); write-back straight from the mapped chunks (the staging
   copy was 130 ms at 16 M); two fetch slots with the host filling the
   next ahead, two write-back slots with the host acknowledging before
   applying; split-loop exits by a jump into the worker instead of a
   signal (57 signals serialised on one lock, 1 ms).
4. The wrapper starts the worker itself and turns unused swap off;
   every thread of the program gets an alternate stack.

At 16 M elements the split loop's run is 4.4 ms; the transport is 35 ms
in and 34 ms out for 64 MiB, within 2x of the link (the explicit path's
20 and 20). What is left there: one DMA per 2 MiB piece, and the mail
round trip between pieces.

Two defects found on the way, both by the test: a stale staging pointer
after a mid-range flush sent write-back entries into the slot the host
was applying (the program's stack was overwritten; visible as garbage
timings), and a fresh huge page's zero-fill landing on the first region.
