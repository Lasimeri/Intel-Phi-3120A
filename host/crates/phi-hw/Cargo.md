# phi-hw

The Xeon Phi as an object on the host: open it through `phi-vfio`, read
POST code and scratchpads, reset, copy into card memory through the
aperture, load and boot a kernel (`boot`), drive one SBOX DMA channel from
host memory (`dma`), and present the ring region as a `phi_ring::RingMemory`
(`ringmem`). Depends on `phi-regs` (register map), `phi-vfio` (device
access, traffic counters), `phi-ring` (region formatting), `libc` (mmap for
DMA buffers), `thiserror` and `log`. See `src/lib.md`.
