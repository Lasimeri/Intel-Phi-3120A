# dma.S: the SBOX DMA engine from the host

One of the eight channels is taken as host owned
(`host/crates/phi-hw/src/dma.rs`; registers and bit layouts from MPSS
`dma/mic_dma_md.c` and `include/mic/mic_dma_md.h`, SSDG 328207-002
2.1.8.2.1): its descriptor ring (256 descriptors of 16 bytes, one page)
lives in pinned host memory, software advances the head pointer
(`DHPR`), the engine advances the tail (`DTPR`). Host memory is reachable
by the card at `SMPT_BASE + iova` once the first SMPT entries map their
16 GiB pages identity (`smpt_identity` in `boot.S`); IOVAs come from
VFIO (`vfio_map_dma`).

**dma_open**: 8 KiB pinned (the ring in the first page, the status word
for copies into host memory in the second); the card-side status slot
cleared (a previous daemon left its last sequence number there); `DCR`
bits for the channel set owner host and disabled while `DRAR_LO`/`DRAR_HI`
(the ring's card address, its size in descriptors, its SMPT page, `SYS`)
are written; both interrupts masked in `DCAR`; the head realigned upward
to a 64-byte line with NOP descriptors when the tail is not on one; then
enabled. A failure logs and returns 0, and the block service falls back
to the aperture.

**dma_submit**: one 64-byte line per copy: the memcpy descriptor (the
40-bit source, the length in 64-byte lines in bits 59:46; the 40-bit
destination with type 1 in bits 63:60), a status descriptor that writes
the sequence number to the status word after the copy (type 2), and two
NOPs; a `mfence` before `DHPR` is written. Addresses and length must be
64-byte multiples, the length under 1 MiB. Up to 63 copies in flight;
`dma_submit` blocks only when the ring is full. Completion is read from
the status words, never from the tail pointer: the engine advanced the
tail before its writes were visible (225 of 10000 rapid copies read back
stale on 2026-09-16). The status word lives in host memory for copies
into host memory and in card memory (read back through the aperture) for
copies into the card, so its arrival implies the data's. A copy that
stays outstanding for 2 s fails the channel.

**dma_poll**, **dma_wait**, **dma_copy** (submit then wait), **dma_close**
(the enable bit cleared) as in the Rust crate. The counters in `traffic`
(bytes and copies, each direction) feed `phictl traffic`.

Verified by the block service's self-test on both channels of card 1
(`disk.md`).
