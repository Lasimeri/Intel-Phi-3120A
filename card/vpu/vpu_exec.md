# vpu_exec.c and vpu_exec.h: the seamless path, card side

The host program never asked for anything: it executed an AVX-512
instruction, the host CPU refused it, and `libphi512`'s handler
(`host/crates/phi512/src/offload.md`) sent a phase of the region around
that instruction here. This engine runs it, physically, on this card's
vector units, and sends back the register file and what the phase wrote.

## What arrives

`struct vpu_exec`: the region's bounds and entry, the code chunk's
address (a 2 MiB chunk of the program's text), a thunk area (entry
stubs, one per thread; the loop-exit jump slot; the host's out-of-line
sequences), the register file (`struct vpu_regs`: zmm0..31, k0..7, the
integer registers in x86 order, flags, rip), the mode, and in ranges
mode the memory ranges the phase touches and the split of its loop. It
is read as the head of one bundle at `VPU_OFF_EXEC_CODE` (descriptor,
thunk area, code pages, contiguous) with one block request; only the two
sizes that fix the bundle's length are read through the uncached
mapping, because reading the descriptor that way was 570 PCIe round
trips, 0.4 ms.

## Memory

Chunks of the program are mapped at the program's own virtual addresses
(`MAP_FIXED_NOREPLACE`), so branches and RIP-relative operands need no
fixing. A chunk is one of 256 huge pages faulted in at start and moved
into place with `mremap` (a fresh huge page costs its zero-fill, about a
millisecond on this core), a placeholder holding its home address
meanwhile. Chunks stay mapped across regions of one program: every
memory-map change flushes the TLB on the 57 CPUs the pool runs on, and
unmapping a region's chunks cost 2 ms. A region's end clears the page
bitmaps (the program changes its memory between regions, so pages are
fetched again) and unmaps only chunks filled by demand faults, which
could not fault again. A new host program (`session` in the descriptor)
drops everything; a dry pool evicts chunks the current region has not
touched.

**Ranges mode.** The pages of every declared range are fetched through
the mailbox before the run, a page once per region, in pieces of up to a
chunk; a range flagged dense (written whole, never read) fetches only
its first and last page. The card alternates two fetch slots and the
host fills the other slot with the next piece of the same range before
it is asked, so the host's copy overlaps the card's DMA. Nothing faults
and no protection changes: an access outside the ranges is a fault the
host hears, and reruns the phase in demand mode. At the exit the ranges
flagged written go back page by page with line masks, straight from the
mapped chunks (a range's pages are contiguous there), alternating two
write-back slots; the host acknowledges a slot on receipt and applies it
after, so the card's next DMA overlaps the apply, and the following
acknowledgement means the slot is free.

**Demand mode.** A SIGSEGV on an unmapped address fetches that chunk
whole and maps it read-only; the first write snapshots it (the pool's 57
threads do the copy) and makes it writable; at the exit the chunk is
compared with its snapshot in 64-byte lines (the pool again) and only
the pages that changed go back, with the lines that changed, so nothing
the host changed meanwhile in the same chunk (its handler's own memory
sits next to the program's arrays) is overwritten with a stale copy.

## Threads

A split loop runs on up to 57 threads of the worker's pool
(`vpu_pool_map`), each with its own register file: the induction
register at its slice's start and the bound register at its slice's
end, except the last thread, which keeps the original bound, runs the
loop's own tail and so leaves the registers and flags the sequential
run would have. Each thread enters through its own stub (rax, then the
entry). The loop exit gets `jmp` to a slot in the thunk area holding
`jmp [rip]` to `vpu_exec_loop_exit` in this binary, which finds the
thread's context through fs (`__thread`), stores the integer registers
and the arithmetic flags (`lahf`, `seto`), switches to the thread's own
stack, stores the vector unit and returns to C: no signal. Fifty-seven
threads taking a signal each went one at a time through the kernel's
per-process signal lock, a millisecond. Every other exit (the region's
own `ud2`, a jump elsewhere, a fault) goes through the signal handlers,
which run on each thread's own stack, execute no vector instruction
(kernel patch 0030 keeps the vector unit across them), record the frame,
and point the interrupted context at `vpu_exec_exit_stub`.

## Exits the host sees

`VPU_EXIT_LEFT` is the normal one. `FAULT`: an address the host has not
mapped, or outside the declared ranges. `ILLEGAL`: a translated
instruction the card refused. `COLLISION`: the program's address is in
use by the worker itself (its binary at 2 MiB, its stacks and pool under
0x7f..; rare, and reported). `LIMIT`: more chunks or snapshots than
tracked. `SPLIT`: a thread of a split loop left elsewhere than the loop
exit. Nothing is written back after any but `LEFT`, so the host can
rerun the phase.

## Measured (2026-09-22 evening, card 0, `tools/avx512-seamless-test.c`)

| elements | polynomial | dot product | integers | emulated polynomial |
| --- | --- | --- | --- | --- |
| 65536 | 7.7 ms | 3.3 ms | 5.5 ms | 22.5 ms |
| 1048576 | 16.2 ms | 10.7 ms | 10.6 ms | 376 ms |
| 16777216 | 98 ms | 139 ms | 91 ms | (about 6 s) |

At 16 M the split loop's own run is 4.4 ms on 57 threads; 64 MiB fetched
in 35 ms and written back in 34 ms, within 2x of the Gen2 x8 link. The
dot product's loop is a reduction, so it runs on one thread (66 ms at
16 M) and stays bit-identical. Every lane of every kernel at every size
matched the host's own FMA3 hardware. `docs/results/2026-09-22-seamless-card.md`.

`vpu_exec_regs.h` holds the register load and store sequences, the same
byte strings as the card kernel's `asm/knc_vpu.h`; regenerate with:

```
awk '/knc_vpu_save/{p=1;next} p&&/asm volatile/{q=1;next} p&&q&&/^\t\t: :/{exit} p&&q{print}' \
    $KERNEL/arch/x86/include/asm/knc_vpu.h   # and the same for knc_vpu_restore; single % in this file
```
