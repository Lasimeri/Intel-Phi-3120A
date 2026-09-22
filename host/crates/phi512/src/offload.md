# offload.rs: the seamless path, host side

From a SIGILL on an AVX-512 instruction, a region of the program runs
on the card's vector units. Nothing is interpreted: the card executes
the program's own instructions, rewritten to its encoding.

## The region

`analyze` walks the code graph from the faulting instruction: every
instruction reachable by falling through and by direct branches, in the
executable mapping that holds the fault. It stops at what the card
cannot run, and those addresses become the region's exits: a call, a
return, an indirect jump, a system call; a VEX or SSE instruction (the
host executes those natively when it resumes); an instruction with a
segment prefix (thread-local storage lives elsewhere on the card); a
legacy instruction outside the card's x86-64 subset (`card_can_run`:
no CMOV, no BMI, no LZCNT, per ISA 327364-001 appendix B.2); an AVX-512
instruction the rewriter refuses. A refusal at the faulting instruction
itself is an error: the program cannot continue and says why.

The 2 MiB chunk of the program around the region is copied whole
(through `process_vm_readv`, so a page that cannot be read is skipped,
not faulted on), the rewrites are overlaid, `ud2` is written at every
exit inside it, and the thunk area is placed in a free stretch of this
process's address space beyond the chunk, within rel32 reach. Regions
are cached by entry address.

## The dispatch

The register file is the frame's integer registers and flags plus the
library's zmm and mask state (`VState`, synced with the real ymm
halves the same way the emulator does). The request goes through the
window (`phi_vpu::window`, `proto::Exec`); while it waits, `run` serves
the mailbox: `MAIL_FETCH` copies a 2 MiB chunk of this process into the
fetch slot, `MAIL_WRITEBACK` applies the pages the card reports changed,
only the 64-byte lines their masks name, through `process_vm_writev`.
On `EXIT_LEFT` the frame gets the card's integer registers, flags and
the exit rip, and the vector state goes back through `VState`. The
other exits are errors with the address that caused them.

The handler runs on an alternate stack (installed at load): the card
writes the program's stack back, and the handler's frame must not be on
it.

## Limits, for now

One region at a time (a mutex). Other threads of the program keep
running meanwhile; a write they make to a line the region also writes is
lost. The region runs on one card hardware thread: the loop split across
the pool is the next step. Every region fetches its chunks afresh.
