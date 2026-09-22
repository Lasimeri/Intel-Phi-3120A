# frame.rs: the machine state a rewritten site hands to the emulator

When a fault is serviced the kernel has already spilled the program's
registers into a signal frame. A rewritten site has no such thing: it
runs in the program's own context with the program's own registers live,
so the trampoline spills them itself, onto its own stack, and passes a
pointer.

`PatchFrame` is that spill, `#[repr(C)]` because the trampoline addresses
it by byte offset: `ymm0` to `ymm15` (512 bytes), `rflags`, then fifteen
general purpose registers in the order the pushes leave them (the
trampoline pushes `r15` first and `rax` last, and the stack grows down,
so `rax` is slot zero). The test `layout_matches_the_stub_generator` pins
every offset; the trampoline test in `patch.rs` pins the instructions
that write them.

## Why it is on the stack

Two threads in the same stub have different stacks and therefore
different frames. Nothing on the hot path needs a lock.

## The register that is not in it

`rsp` cannot be pushed like the others: by the time the trampoline could
push it, it has already moved. `PatchCpu` reconstructs it from the
frame's own address, because the distance between them is fixed by the
stub and the trampoline: the frame itself, the return address the `call`
pushed, and the 128-byte red zone the stub stepped over. That is
`RSP_ABOVE_FRAME`, 776 bytes, and it is the value an `rsp`-relative
memory operand in the emulated instruction computes with.

## The registers that are real

The `ymm` half of the frame is not a copy of anything: on this host the
low 256 bits of what the program believes are `zmm` registers are the
real `ymm` registers. They are read into the emulator's view before the
instruction and written back after, through the same `sync_in_masked`
and `sync_out_masked` the fault handler uses, restricted to the
registers the instruction names.
