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
