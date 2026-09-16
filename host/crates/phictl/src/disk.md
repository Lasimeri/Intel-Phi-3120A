# disk.rs

The host end of the card's block device (`/dev/phiblk0`, kernel patch
0025): `phictl boot --disk PATH` runs this on its own thread next to the
console, the control socket and the port forwarder.

Protocol (ring channel kind 4, `docs/spec/ring-protocol.md`): the card
writes 32-byte request records into the card-to-host ring, little-endian:

| offset | field | meaning |
| --- | --- | --- |
| 0 | tag u32 | echoed in the completion; the card encodes request and segment |
| 4 | op u32 | 0 read, 1 write, 2 flush, 3 identify |
| 8 | len u32 | bytes, a multiple of 512, at most 65536 |
| 12 | reserved u32 | 0 |
| 16 | sector u64 | start sector (512 bytes) |
| 24 | phys u64 | card physical address of the buffer, 64-byte aligned |

The host answers each record with an 8-byte completion in the
host-to-card ring: tag u32, status u32 (0 ok, else an errno: 5 for an
I/O error, 22 for a bad record, 95 for an unknown operation). For
identify the status is the capacity in sectors; a capacity of 0 means no
disk. Data moves with the DMA engine when `open_path` brings it up (channel 0,
ring and a 512 KiB buffer pinned in host memory, `phi-hw/src/dma.rs`) and
passes a self-test through card memory the card does not use (the last
MiB of the ring region): a read is `pread` into the pinned buffer then one
DMA descriptor into the card address the record names; a write is the
reverse. The identify answer then carries tag `0xfffffffe`, and the card
hands its pages over directly. Without the engine (`--no-dma`, or a
failed self-test) the host copies through the aperture and answers with
tag `0xffffffff`, and the card bounces through its uncached slots, since
aperture writes are not seen by the card's caches (measured 2026-09-16,
`docs/results/2026-09-16-storage.md`). Flush is `fdatasync`. Set
`PHICTL_DISK_TRACE=1` to log the first 40 records.

Records are served one at a time in order. The thread spins for 3 ms
after the last record, then polls every 200 us; the same policy runs on
the card. The DMA engine will replace the aperture copies (same
records, same addresses, 64-byte alignment already required).

Security: the image is a plain file opened read/write by the user who
booted the card; nothing else on the host touches it. Keep it mode 0600.
