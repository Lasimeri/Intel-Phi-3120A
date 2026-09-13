# phi-ring / layout.rs

The wire format as offset constants. The C mirror is
`card/drivers/phinet/include/phi_ring.h`, and `tools/ring-layout-check.c`
(compiled with `tcc`) prints the C `offsetof`/`sizeof` values so the two can
be diffed by eye or by script.

## Why offsets instead of structs

The bytes live in card memory reached through an uncached BAR. Building a
Rust struct over them would invite ordinary loads and stores (and the
compiler's freedom to merge them). Offsets plus `RingMemory` keep every
access explicit and volatile in the hardware backend.

## Cache-line discipline

`head` is written only by the producer, `tail` only by the consumer, and
each sits alone on a 64-byte line so that neither side's writes invalidate
the other's line. The 64-byte figure is the KNC L1/L2 line size (SSDG 2.1.1).
