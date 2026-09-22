# patch.rs: rewriting a faulting site so it never faults again

A fault costs about 1909 ns on this host and the arithmetic behind it
under 200 (`docs/research/avx512-transparency.md`), so nine tenths of
the price of emulating an AVX-512 instruction is the processor saying
it happened. This module pays that once per site instead of once per
execution.

| | ns per AVX-512 instruction, measured 2026-09-22 |
| --- | --- |
| fault on every execution | 2158 |
| site rewritten | **152** |
| of which stub and trampoline | 70 |
| of which emulation | 100 |

## Why a jump fits

An EVEX instruction is at least six bytes (`62H`, three payload bytes,
opcode, ModRM); a near jump is five. So the site is overwritten with
`jmp stub`, a `nop` fills what is left, and the stub steps over the red
zone, calls the one trampoline, steps back, and jumps to the instruction
after the original.

The exception is the mask register family: `kmovw` and its relatives
are AVX-512F but **VEX** encoded and four bytes long. No jump fits. They
keep faulting, counted in `TOO_SHORT` and named under `PHI512_VERBOSE`.
In mask-heavy code they were 99.8 percent of the remaining faults
(819200 of 821105 in a 16-thread test), which is the cost still on the
table.

## The trampoline

One copy, hand-assembled at the start of an executable arena mapped
within jump range of the first faulting site. It pushes the fifteen
general purpose registers (`push` does not touch the flags), then
`pushfq`, then makes room and saves `ymm0` to `ymm15`. Every stack
adjustment before `pushfq` is `lea`, never `sub`, because an AVX-512
instruction leaves the flags alone and the program may branch on them
next. `nothing_before_pushfq_touches_the_flags` enforces that.

It then calls `phi512_patched` with the frame and the return address.
The return address identifies the site: stubs are a fixed 32 bytes in
one arena, so the index is a division. Passing an index in a register
would have clobbered a register before anything had saved it.

`the_trampoline_disassembles_to_what_it_should` checks all 74
instructions by disassembly. A wrong byte here would not produce a wrong
answer; it would produce a crash in somebody else's program with a stack
that makes no sense.

## Rewriting while other threads run

Three things had to be true, each found by a crash:

1. **One rewriter at a time.** Two threads faulting on the same
   instruction both tried to rewrite it and their five-byte stores
   interleaved. `PATCH_LOCK` is taken to rewrite; a loser does nothing,
   having already emulated its instruction.
2. **The site is published before the breakpoint goes in.** The rewrite
   is the kernel's own three-step: `int3` over the first byte (one byte,
   atomic), the rest of the jump, then the first byte becomes `e9`. A
   thread arriving mid-rewrite traps, and the `SIGTRAP` handler has to
   recognise the address as ours, wait for `Ready`, and re-execute the
   site. If the site were published afterwards the trap would look
   foreign and the process would die.
3. **A fault can outlive its instruction.** Between the processor
   raising `SIGILL` and the handler reading the bytes, another thread may
   have finished the rewrite, so the bytes are a `jmp`. The handler
   re-checks the site table *after* decoding, not only before, and
   returns without moving `RIP` so the site re-executes as the jump.

A site that was published and then could not be made writable is marked
`Failed`, not left `Patching`: a thread waiting on it would otherwise
spin for the rest of the process's life. Its instruction is still the
original and keeps faulting, which is slower and correct.

## No heap in the handler

The stub builder runs inside the `SIGILL` handler, and the thread that
faulted may have been inside the allocator holding its lock. Code is
assembled into a fixed 512-byte buffer (`Code`), the site table and the
decoded-instruction cache are statics, and the per-thread register file
is `const`-initialised.

## What a site costs per execution

`phi512_patched` runs in the program's context, so it may allocate and
lock, and does neither. The instruction was decoded once, when the site
was rewritten, and cached; the registers it names were worked out then
too, so only those are moved between the frame and the emulator's view.
If the emulator refuses an instruction it performed successfully from
the fault handler, the process is stopped with a message rather than
continuing past a register the program believes was written: skipping
silently is the one thing this library must never do.
