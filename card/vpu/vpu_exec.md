# vpu_exec.c and vpu_exec.h: the seamless path, card side

The host program never asked for anything: it executed an AVX-512
instruction, the host CPU refused it, and `libphi512`'s handler
(`host/crates/phi512/src/offload.rs`) sent the region of the program
around that instruction here. This engine runs it, physically, on this
card's vector units, and sends back the register file and what the
region wrote.

## What arrives

`struct vpu_exec` at `VPU_OFF_EXEC` in the window: the 2 MiB chunk of
the program's text holding the region (its AVX-512 instructions
rewritten to MVEX in place, `ud2` at every exit), a thunk area (out-of-
line sequences for the few instructions the card lacks, and the entry
stub), the region's bounds, and `struct vpu_regs`: zmm0..31, k0..7, the
integer registers in x86 order, flags and rip.

## What happens

1. The code chunk and the thunk area are mapped at the program's own
   virtual addresses (`MAP_FIXED_NOREPLACE`), so branches and RIP-relative
   operands need no fixing. A huge page backs a chunk when the card has
   one (`scripts/phi-vpu.sh start` reserves them): one physical run, four
   block records to move instead of 512.
2. `vpu_exec_enter` saves the dispatcher's callee-saved registers and
   stack, loads the register file (vector unit first, then flags, then the
   integer registers, rsp last) and jumps to the entry stub, which sets
   rax, the one register the loader had no hands left for, and jumps to
   the faulting instruction.
3. The rest of the program's memory arrives as the code touches it. A
   SIGSEGV on an unmapped address asks the host for that 2 MiB chunk
   through the mailbox (`struct vpu_mail`: the card writes addr, len,
   kind, then seq; the host serves and writes ack), maps it read-only at
   the same address and retries. The first write to a chunk faults again:
   the chunk is snapshotted, made writable, and marked dirty.
4. Any instruction fetch outside the region and the thunk area is the
   exit: the `ud2` the host placed, or a jump elsewhere. The SIGILL (or
   SIGSEGV on a non-executable chunk) handler records the exit rip, the
   integer registers and flags from the frame, and points the interrupted
   context at `vpu_exec_exit_stub` on the dispatcher's stack; after
   sigreturn, with the region's vector registers live again (kernel patch
   0030), the stub stores zmm and k and returns to C.
5. Every dirty chunk is compared with its snapshot in 64-byte lines, and
   only the pages that changed go back, each with the mask of its changed
   lines (`struct vpu_wb_page`, `VPU_MAIL_WRITEBACK`), so nothing the host
   changed meanwhile in the same chunk (its handler's own memory sits next
   to the program's arrays) is overwritten with a stale copy. Then
   everything is unmapped.

The signal handlers run on their own stack and execute no vector
instruction. `vpu_exec_regs.h` holds the register load and store
sequences, the same byte strings as the card kernel's `asm/knc_vpu.h`.

## Exits the host sees

`VPU_EXIT_LEFT` is the normal one. `FAULT`: the region touched an
address the host has not mapped (the program would have crashed there).
`ILLEGAL`: a translated instruction the card refused. `COLLISION`: the
program's address is in use by the worker itself (its binary at 2 MiB,
its stacks and huge buffers under 0x7f..; rare, and reported rather
than hidden). `LIMIT`: more chunks or snapshots than tracked.

## Measured (2026-09-22, card 0, `tools/avx512-seamless-test.c`, 65536 elements)

First working version: the polynomial region (two nested loops, 15
instructions) 32 ms end to end, of which chunk fetches 9.5 ms (five
2 MiB chunks), the region with its faults and snapshots 16.5 ms, the
line diff and write-back 9.3 ms. All lanes bit-identical. The costs
named there are the next work: pre-faulted huge pages (the card kernel
zero-fills a fresh one), the pool doing copies and diffs, the loop
split across the pool. `docs/results/2026-09-22-seamless-card.md`.

Regenerate `vpu_exec_regs.h` when the kernel header changes:

```
awk '/knc_vpu_save/{p=1;next} p&&/asm volatile/{q=1;next} p&&q&&/^\t\t: :/{exit} p&&q{print}' \
    $KERNEL/arch/x86/include/asm/knc_vpu.h   # and the same for knc_vpu_restore; single % in this file
```
