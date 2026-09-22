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
| 8 | len u32 | bytes, a multiple of 512, at most 524288 (`MAX_LEN`, the card driver's request size) |
| 12 | reserved u32 | 0 |
| 16 | sector u64 | start sector (512 bytes) |
| 24 | phys u64 | card physical address of the buffer, 64-byte aligned |

The host answers each record with an 8-byte completion in the
host-to-card ring: tag u32, status u32 (0 ok, else an errno: 5 for an
I/O error, 22 for a bad record, 95 for an unknown operation). For
identify the status is the capacity in sectors; a capacity of 0 means no
disk. Data moves with the DMA engine when `open_path` brings it up (one channel
per served device, ring and a 512 KiB staging buffer pinned in host
memory, `phi-hw/src/dma.rs`) and passes a self-test through card memory
the card does not use (the last 2 MiB of the ring region): a read is
`pread` into the staging buffer then one DMA copy into the card address
the record names; a write is the reverse; host memory (`--host-mem`) is
copied straight between the window and the card's pages. Without the
engine (`--no-dma`, or a failed self-test) the host copies through the
aperture. Both paths are coherent with the card's caches (measured
2026-09-16), so the identify answer carries tag `0xfffffffe` and the card
hands its pages over directly; `PHICTL_DISK_BOUNCE=1` answers `0xffffffff`
instead, for the bounce path. Flush is `fdatasync`. `PHICTL_DISK_TRACE=1`
logs the first 40 records; `PHICTL_DMA_STRESS=N` runs N verified random
copies at start.

## Pipelining (2026-09-22)

A read or write of host memory on the DMA path is submitted to the
engine as its record arrives and answered when its copy completes, in a
later pass of the loop, up to `MAX_IN_FLIGHT` (63) at once; everything
else (an image file, the aperture path, identify, flush) is served one
at a time in order after the pipeline has drained, so a flush never
overtakes a write. The reason is the record count: in direct mode the
card posts one record per physically contiguous run of the request's
pages, so a 512 KiB request from a 4 KiB-paged buffer arrived as 88
records, and served one at a time each cost the round trip. Measured on
card 0 (`docs/results/2026-09-22-block-pipeline.md`): 64 MiB into the
card went from 43 ms to 21 ms, out from 23 ms to 20 ms, the link.

The thread keeps polling for 20 ms after its last record (the gaps
between one program's requests), then sleeps 200 us between polls: the
sleep and its wake-up are 0.25 ms on the first record of the next burst,
most of the transport of a small request. The card's driver does the
same on its side (kernel patch 0029).

Security: the image is a plain file opened read/write by the user who
booted the card; nothing else on the host touches it. Keep it mode 0600.
