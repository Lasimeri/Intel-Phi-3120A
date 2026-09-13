# phi_ring.h

C mirror of the ring layout. Field order and padding reproduce
`host/crates/phi-ring/src/layout.rs` exactly; the `_Static_assert`s pin the
three structure sizes to the 64-byte lines the spec requires, and
`tools/ring-layout-check.c` prints every `offsetof` for comparison with the
Rust constants (`region_hdr::*`, `channel_desc::*`, `ring_hdr::*`).

Usable from both the kernel module (`__KERNEL__`) and host-side C helpers.
