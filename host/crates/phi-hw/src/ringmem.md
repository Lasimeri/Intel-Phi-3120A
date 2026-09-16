# phi-hw / ringmem.rs

Adapts the aperture mapping to `phi_ring::RingMemory`. The interesting
part is `fence`: on the host the ring protocol's "make my data visible
before I publish the index" step is a read from the same BAR, which PCIe
ordering rules guarantee cannot return before earlier posted writes have
been accepted by the device. This is the same trick Intel's driver used
(`wmb()` followed by an MMIO read in `mic_x100_send_firmware_intr`).

Region base and length are fixed at construction and every access is
bounds-checked against them, so a protocol bug cannot scribble outside the
1 MiB the kernel reserved.

## Index accesses (2026-09-16)

`read_u32` and `write_u32` are implemented with the mapping's single
32-bit accessors. The trait's defaults go through `read`/`write`, and the
aperture byte copy splits a 4-byte request into four byte accesses (its
8-byte body loop needs at least 8 bytes), so a ring head or tail published
by the host could be observed torn by the card: a head moving from 0x00ff
to 0x0100 passed through 0x0000, the card computed a wrapped byte count
and consumed zeros as records. Seen 2026-09-16 as "completion for idle
tag 0 status 0" floods and a hung block device; the older channels had
hidden it behind their resynchronisation guards.
