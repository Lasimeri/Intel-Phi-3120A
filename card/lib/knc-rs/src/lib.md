# lib.rs (knc)

Rust bindings to `libknc` (`card/lib/knc/knc.md`), for Rust that runs on
the card.

## Why bindings and not intrinsics

rustc cannot emit Knights Corner vector instructions. `core::arch` has no
MVEX intrinsics, no compiler has emitted that encoding since Intel's dead
k1om toolchain, and the project's patched LLVM adds only the *absence* of
instructions the card lacks, not the presence of the ones it has
(ADR 0002, ADR 0007). Inline `asm!` with `.byte` blocks would work and is
what `host/crates/knc-mvex` exists to produce, but doing it per caller
would mean every Rust program on the card carrying its own copy of 20000
lines of generated assembly.

So Rust reaches the vector unit the same way C++ and Python and JavaScript
do: through one shared static library whose kernels are generated once.

## What the crate adds over `extern "C"`

Two things the C ABI cannot express.

**Alignment.** Every kernel loads and stores whole 64-byte vectors and
there is no unaligned form to fall back to, so an arbitrary `&[i32]` is not
a valid argument. `Block` and `Packed` are `#[repr(C, align(64))]`, which
makes the alignment a property of the type instead of a comment.

**Width.** `libknc`'s C entry point returns silently for a width outside 1
to 32, because a computed jump is the alternative and the library is called
from languages where the width comes from data. `Codec::new` turns that
into a `Result` once, so the pack and unpack calls cannot fail and do not
branch.

`Packed` is always 4 KiB, the size of the widest block. A narrower width
uses `Codec::words()` of it and leaves the rest. That trades memory for not
having a const generic in every signature; a caller packing millions of
blocks should allocate `Codec::packed_bytes()` itself and use the raw
`extern "C"` entry points.

## `no_std`

The crate needs nothing from `std`. The demo binary in `main.rs` does.

## Measured, 2026-09-20, decode

| bits | 1 thread | 228 threads |
| --- | --- | --- |
| 1 | 372.6 | 11045.3 |
| 8 | 276.9 | 10528.6 |
| 11 | 270.2 | 9650.4 |
| 16 | 259.4 | 9607.0 |
| 32 | 231.3 | 8510.2 |

Millions of values per second. Below the C++ figures in
`docs/results/2026-09-20-libknc.md` (12556 at 11 bits) because `Packed` is
4 KiB whatever the width, so the working set at 11 bits is three times
larger than it needs to be and the memory bound arrives sooner. The kernels
are identical; this is the cost of the simpler type.
