# ring.S: the ring transport region

The region of card memory the host and the card share
(`docs/spec/ring-protocol.md`), reached through the aperture at
`region_base`; every offset here is region-relative
(`host/crates/phi-ring` and `phi-hw/src/ringmem.rs`).

**Memory.** `rm_read32`/`rm_write32` are single 32-bit accesses, which is
what the ring indices require: a byte-wise index publish was read torn
by the card (completions for tag 0 with status 0 and a hung block device,
2026-09-16). `rm_read`/`rm_write` are the aperture copies. `rm_fence` is
one read of the region: a PCIe read cannot complete before this CPU's
earlier posted writes reach the device, so an index published after it
follows its data.

**Rings.** Single producer, single consumer, free-running u32 indices,
data at `index & (size - 1)`, `head - tail <= size` always, so the
arithmetic is exact across the wrap. `ring_push` writes the bytes (in
two pieces around the end when needed), fences, publishes `head`;
`ring_pop` reads, fences, publishes `tail`. Neither end caches an index:
every operation reads both, so an end attached before the other side
reset follows it. `ring_free` reads a tail ahead of the head as a full
ring; `ring_avail` and `ring_pop` treat `head - tail > size` as
inconsistent (the producer reset): `ring_pop` sets `tail = head` and
drops what was there. The block services read the indices themselves
instead (`disk.md`), because a dropped block request hangs the card.

**region_open**: magic `PHIR`, version 1, the recorded size within the
mapped one, the channel table within the size; each known channel's two
rings 64-byte aligned, power-of-two sized and inside the region, its
data area page aligned and inside; unknown kinds skipped. `endpoints_of`
gives a kind's host endpoints (a producer on the host-to-card ring, a
consumer on the card-to-host one) after checking both ring headers.

**region_format** lays the default plan out in host memory: console
(4 KiB in, 64 KiB out), network (256 KiB each way), rpc (256 KiB each
way), block (16 KiB in, 64 KiB out, an 8 MiB data area), host memory
(16 KiB in, 64 KiB out); rings after the table on 64-byte boundaries,
data areas after the last ring on page boundaries; the header with the
host's wall clock, the host memory window, and the magic written last.
`plan_end` gives where the layout ends (the block services' self-test
scratch and status words live above it, in the last 2 MiB).

The layout is the one `phi_hw::boot::DEFAULT_CHANNELS` produced; the
card kernel's ring drivers read the descriptors, not a fixed layout.
