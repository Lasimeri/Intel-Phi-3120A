# Performing an AVX-512 instruction without an AVX-512 processor

Everything here works on the imaginary register file (`state.md`) and on
the program's own memory, which is reachable because this runs inside the
program's address space.

## Why scalar Rust and not host vector intrinsics

These are IEEE operations with the same rounding as the hardware, so lane
by lane in scalar arithmetic gives the same bits as a vector unit would,
and it is much easier to read and to be sure of.

The performance answer is that this code is not meant to be the fast path.
The plan is to stop reaching it, by rewriting each call site once its
translation exists, rather than to make the fallback clever. A vectorised
fallback would be maybe 6x faster and considerably harder to trust, and it
would still be 300x off the target.

One choice here is not free. The fused multiply-adds use `mul_add`, which
is a single rounding, because that is what the hardware FMA does. Writing
`a * b + c` would round twice and produce different bits. The test
`fma_rounds_once_like_the_hardware` constructs values where the two
disagree by construction rather than by luck, so an implementation that got
this wrong could not pass.

## The shapes an instruction can have

**Two sources, one destination**, which is most of them. The second source
is a register or memory, and if the broadcast bit is set a memory source
names *one element* rather than a vector: `{1to16}` reads four bytes and
uses them for all sixteen lanes. That is why the source is materialised
through `source_bytes` rather than copied.

**The destination is also a source** for the `132`, `213` and `231` forms
of the multiply-adds, which is why it is captured before any lane is
written. Writing lane 0 and then reading the destination for lane 1 would
be correct here only by accident of lane ordering; capturing it up front is
correct regardless.

**Moves**, where one operand is memory and there is no arithmetic. A masked
store writes only the enabled lanes and leaves the rest of memory alone,
and there is no zeroing form of a store, because zeroing masking describes
what happens to a register's lanes.

## Two rules that are easy to miss and corrupt state silently

**A vector write zeroes everything above the width it wrote.** The 128-bit
and 256-bit EVEX forms write 16 or 32 bytes and clear the rest of the
512-bit register. Ignoring that would leave stale data in the upper lanes,
which the next full-width instruction would then read as if it were real.

**Masking is merging unless `{z}` is present.** A disabled lane keeps the
destination's previous value; it is not zero and it is not undefined.

## Unsupported means error, never skip

An instruction with no implementation returns `Unsupported`, and the
handler stops the program. Skipping it would leave the imaginary register
file disagreeing with what the program believes it computed, and every
answer afterwards would be wrong with nothing to indicate it. A program
that stops with "no emulation for Vpconflictd" is a program whose author
can do something about it.
