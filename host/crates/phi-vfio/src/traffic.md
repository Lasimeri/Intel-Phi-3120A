# traffic.rs: bytes moved over PCIe

Four process-wide counters, one per path and direction: the DMA engine
into the card, the DMA engine into host memory, the aperture (BAR0)
written by the host CPU, the aperture read by it. `Mapping::read_bytes`
and `write_bytes` add to the aperture pair, `DmaChannel::copy` in
`phi-hw` to the DMA pair, so everything the daemon moves is counted where
it moves: the ring channels (console, network, rpc), the disk and host
memory services, the kernel and initramfs load.

Not counted: 32-bit register accesses (ring indices, SBOX registers, the
POST code). They are polling and control, a few kilobytes per second at
most, not payload.

`phictl traffic` prints the counters; `phitop` differences them into
rates. The counters are `Relaxed` atomics: they are statistics, not
synchronisation.
