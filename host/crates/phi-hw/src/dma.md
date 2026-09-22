# dma.rs

The card's SBOX has eight DMA channels (SSDG 328207-002, 2.1.8.2.1). This
module drives one from the host, which the MPSS host driver also did:

- `HostDmaBuffer`: an anonymous, populated mapping pinned and mapped for
  the card through VFIO type1 at a chosen IOMMU address. With the system
  memory page table (SMPT, 32 entries of 16 GiB) mapped identity, the card
  reaches IOMMU address `x` at card address `0x80_0000_0000 + x`
  (`smpt_identity`, as `mic_smpt_init` in MPSS).
- `DmaChannel::new`: marks the channel host owned in `DCR`, points `DRAR`
  at a 4 KiB ring of 256 descriptors in host memory (SYS bit, SMPT page
  bits, size), masks both interrupt paths in `DCAR`, sets the head equal to
  the current tail and enables the channel.
- `DmaChannel::submit`: writes one cache line of descriptors at the head:
  the memcpy (40-bit source and destination card addresses, length in
  64-byte lines, type 1), a status descriptor (type 2) that stores the
  copy's sequence number, and two NOPs; advances `DHPR` by four and
  returns the sequence number. Up to `MAX_IN_FLIGHT` (63) copies sit in
  the ring at once; a submission into a full ring waits for the oldest.
- `DmaChannel::poll`: the sequence number of the last copy done, from
  the status words: one in host memory for copies into host memory, one
  in card memory (read through the aperture) for copies into the card,
  read only while such a copy is outstanding. Sequence numbers only
  grow and the engine runs the ring in order, so the larger word is the
  last copy done and everything before it is done too. `wait(seq)`
  spins on `poll`; `copy` is `submit` then `wait`, one at a time.
  The tail pointer is not a completion signal: the engine advances
  it before its writes are visible (measured 2026-09-16, 225 of 10000
  rapid copies stale), which is why MPSS polls a status word too. Two
  seconds without progress is an error reported with `DTPR`, `DCHERR`
  and `DSTAT`. `DCR` updates are serialised across channels. `new`
  zeroes the card-side status word: it outlives the process, and a
  previous daemon's last sequence number would read as completion.

Splitting submit from wait is what lets the block services keep the
engine busy (`docs/results/2026-09-22-block-pipeline.md`): served one
copy at a time, 64 MiB into the card took 43 ms; pipelined, 21 ms, the
Gen2 x8 link.

Constraints from the hardware: 64-byte alignment and granularity, at most
1 MiB minus 64 bytes per descriptor, rings aligned to their size. The
engine's accesses to host memory are snooped (SMPT bit 0 clear).

Register offsets and descriptor bit positions are in `phi-regs` (`sbox`),
taken from MPSS `dma/mic_dma_md.c` and `include/mic/mic_dma_md.h`
(vendor tree, reference only). Used by `phictl boot --disk` for the block
device's data path; see `docs/results/2026-09-16-dma.md` for the numbers.

## Traffic counters (2026-09-17)

A submitted copy adds its length to `phi_vfio::traffic::DMA_FROM_CARD`
when the destination is host memory and to `DMA_TO_CARD` when the
source is, and one to the matching copy counter (`DMA_COPIES_FROM_CARD`,
`DMA_COPIES_TO_CARD`, 2026-09-22), so bytes over copies is the mean
copy size, which is what the block path is bound by; a copy within card
memory (the self-test's pattern moves) counts as neither. `phictl
traffic` and `phitop` read the counters.
