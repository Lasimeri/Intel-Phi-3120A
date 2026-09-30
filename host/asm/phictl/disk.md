# disk.S: the block services

`--disk IMG` is the card's `/dev/phiblk0` (ring channel kind 4, kernel
patch 0025), `--host-mem SIZE` its `/dev/phiblk1` (kind 5) backed by the
pinned window that is also `/dev/phihost`. The protocol
(`docs/spec/ring-protocol.md`, `host/crates/phictl/src/disk.md`): the
card posts 32-byte request records in the card-to-host ring (tag, op,
length, reserved, sector, the card physical address of the buffer); the
host answers each with an 8-byte completion (tag, status: 0, or an errno
of 5, 22 or 95; the capacity in sectors for identify). Data moves
through the DMA engine (`dma.S`), or through the aperture when the
engine is unusable or `--no-dma` was given.

**Bring-up** (once the card has reached init): the rings; the backend
(the image opened read/write, its size in sectors; or the host memory
window); the data path: the engine opened on the service's channel
(channel 0 for the disk, 1 for host memory, each with its own descriptor
ring, staging buffer and status slot in the last 4 KiB of the region)
and proved by a round trip through card memory the card does not use
(the last 2 MiB of the region): 64 KiB of a pattern into the card by DMA
and read back through the aperture, the inverse pattern in through the
aperture and out by DMA, then sixteen round trips of 512 KiB for the
rate. Anything failing leaves the aperture path with the reason logged.

**The pass** (`blk_step`, up to 64 records per turn): copies the engine
finished are answered oldest first; the indices are read raw (an
impossible count is logged and re-read, never resynchronised: a dropped
block request hangs the card); a record is popped. Host memory on the
engine, read or write: submitted at once and answered when its copy
completes in a later pass, up to 63 in flight (the card posts one record
per physical segment, hundreds for a 16 MiB request, and the round trip
per record is what a serialised service is bound by,
`docs/results/2026-09-22-block-pipeline.md`). Everything else (the
image, the aperture path, identify, flush) is served one at a time in
order after the pipeline has drained, so a flush never overtakes a
write. Identify answers tag `0xfffffffe` (the card hands its pages over
directly; both host paths are coherent with its caches, measured
2026-09-16) unless `PHICTL_DISK_BOUNCE` is set. Flush is `fdatasync` of
the image. `PHICTL_DISK_TRACE` logs the first 40 records. Reads of the
image go `pread` into the staging buffer then one DMA copy to the record's
address; writes the reverse; on the aperture path through a host buffer.

Not ported: `PHICTL_DMA_STRESS=N` (the Rust service's random-copy
self-test), a diagnostic of the engine that the DMA results of
2026-09-16 record.

**Measured on card 1** (`docs/results/2026-09-29-phictl-assembly.md`):
the self-test 1652 MB/s on both channels; `/data` read 350 MB/s after a
cache drop (the Rust daemon 326), 64 MiB of zeros written and synced at
195 MB/s (196), sha256 identical before and after; host memory with swap
off, 32 MiB written at 130 MB/s and read at 159 MB/s (170),
byte-identical; a clean unmount on shutdown, the image clean.
