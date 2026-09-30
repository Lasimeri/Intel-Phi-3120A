# phi_ring.h

C mirror of the ring layout. Field order and padding reproduce
`host/asm/phictl/defs.inc (the RH_, CD_ and RG_ constants)` exactly; the `_Static_assert`s pin the
three structure sizes to the 64-byte lines the spec requires, and
`tools/ring-layout-check.c` prints every `offsetof` for comparison with the
Rust constants (`region_hdr::*`, `channel_desc::*`, `ring_hdr::*`).

Usable from both the kernel module (`__KERNEL__`) and host-side C helpers.
