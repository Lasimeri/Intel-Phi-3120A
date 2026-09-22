# Catching the processor's refusal

Installed from `.init_array`, so `LD_PRELOAD` is the whole integration and
the program needs no cooperation.

## What the handler does

Reads `RIP` out of the signal frame, decodes the instruction there, performs
it against the imaginary register file, and sets `RIP` past it so the
program resumes at the next instruction.

The three properties this depends on were measured before the code was
written (`docs/research/avx512-transparency.md`): the fault is synchronous,
the saved `RIP` points **at** the faulting instruction rather than past it,
and changing `RIP` and returning resumes cleanly.

## Reading the program's registers

Memory operands are computed from live register values, so the emulator
needs them, and they are in the signal frame rather than in registers by
the time the handler runs. `Frame` reads them out of
`uc_mcontext.gregs`, whose ordering is glibc's and is spelled out in the
constants at the top of the file.

`full_register()` maps `eax` and `ax` onto `rax`, because they are the same
machine register and an address computation uses all of it.

## Why the output routines look like that

This is a signal handler, so it cannot allocate and cannot take a lock.
`println!` does both. `say` and `num` write into fixed stack buffers and
call `write(2)` directly. They are ugly on purpose; the alternative is a
handler that deadlocks against the allocator in exactly the situation
someone is trying to debug.

## What happens when it cannot help

Two cases, both handled the same way: the faulting instruction is not
AVX-512 at all (so the fault is genuinely the program's), or it is AVX-512
but not in the emulator's table. In both, the handler restores `SIG_DFL`
and returns, so the process dies exactly as it would have without this
library loaded, after printing which instruction it was.

That is deliberate. A layer like this must not convert a program's own bug
into a hang, and it must not paper over its own gaps.

## Known limits

- The state is per thread and starts zeroed in each thread, which matches
  how a thread's vector state actually begins.
- `thread_local!` with `const` initialisation is used so that first touch
  inside a signal handler does not allocate.
- After the instruction has been performed once, `patch::try_patch` rewrites
  the site so it never faults again (`patch.md`). Only sites shorter than a
  five-byte jump, the VEX-encoded mask instructions, keep faulting.

## The card path (2026-09-22)

At load, unless `PHI512_EMULATE` is set, the handler opens the card's
window (`offload::init`) and installs an alternate signal stack for the
thread. A SIGILL on an AVX-512 instruction then goes to `offload::run`,
which returns with the frame at the region's exit; the emulator, site
rewriting and breakpoints are not used at all in that mode. Without a
card and without `PHI512_EMULATE`, the first AVX-512 instruction ends
the program with the reason: nothing is interpreted silently. The exit
report says how many regions the card ran.
