# proto.rs: the contract, from the host's side

The Rust mirror of `card/vpu/vpu_proto.h`: window offsets, the readiness
magic, the kernel numbers, the block and chunk sizes, and the request and
reply structures as `#[repr(C)]`.

Nothing checks the layout at run time, so three things check it before:

- both files assert the two structure sizes at compile time (56 and 48)
- the unit tests here pin every field offset with `offset_of!`
- `tools/vpu-layout-check.c` prints the C compiler's offsets against the
  same numbers

`status_name` turns a reply status into words, so a failure says what
went wrong rather than printing a small negative number.

## The seamless path (2026-09-22 evening)

`Exec`, `Regs`, `Range`, `Mail` and `WbPage` mirror `card/vpu/vpu_exec.h`
(v2: modes, phases, ranges, the split, two fetch and two write-back
slots, the bundle at `OFF_EXEC_CODE`); `tools/vpu-layout-check.c` checks
the offsets. `K_EXEC` is the request kind.
