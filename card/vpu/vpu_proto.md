# vpu_proto.h: the host/card contract

One window of host memory, seen from both sides. The host maps
`/dev/shm/phi-hostmem`. The card reaches the same bytes two ways, and
which one to use for what is a measurement, not a preference:

| path | measured 2026-09-22 | used for |
| --- | --- | --- |
| `/dev/phihost`, mapped, uncached | 2.36 us doorbell round trip; 50 MB/s streaming | control words |
| `/dev/phiblk1`, DMA block device | 1.2 GB/s at 16 MiB reads, 553 MB/s at 4 MiB, 184 MB/s at 1 MiB | bulk data |

That the two paths address the same bytes at the same offsets was
verified with a marker: written by the host at window offset 1 MiB, read
back identically through the block device at the same offset.

## The doorbell measurement, and why the first one was wrong

The first doorbell measurement reported 1.4 us while the card was not
answering at all: the card's counter still held a large value from an
earlier run, so the host's wait was satisfied instantly and it timed
nothing. The 2.36 us figure is trustworthy because its control case
fails (a host with no responder reports that the card never signalled
ready and exits non-zero) and because the card independently reports how
many doorbells it answered (5000 sent, 5000 answered). Any measurement
that stale memory can satisfy needs a control that fails.

## Layout

| offset | what |
| --- | --- |
| 0 | readiness word: the card writes `VPU_MAGIC` while it is polling |
| 64 | `struct vpu_request`, 56 bytes |
| 256 | `struct vpu_reply`, 48 bytes |
| 1 MiB | bulk data |

Each control word is on its own 64-byte line so the two sides never
share one. The host writes the request's fields, then its sequence
number last; the card polls only the sequence number, and writes its
reply's fields before echoing the number, which is all the host polls.

The reply carries four timings: the vector units alone, moving data in,
moving data out, and the whole request from doorbell to reply. Reporting
them separately is what showed the transport, not the compute, to be
where a request's time goes.

## Alignment rules

All consequences of the card moving data with `O_DIRECT`:

- `in_off`, `out_off` and `aux_off` are multiples of 4096
- the card transfers whole blocks, so every region occupies whole blocks
  and nothing else may live in the slack after it
- the POLY30 kernel works in steps of 128 elements; elements past `n` up
  to the next multiple of 128 are computed on whatever the slack holds
  and written back as garbage inside the output's last block

A violation is reported as status -3 or -4, never silently.

## The Rust mirror

`host/crates/phi-vpu/src/proto.rs` is the same contract for the host.
Both files assert the two structure sizes at compile time, the Rust unit
tests pin every field offset, and `tools/vpu-layout-check.c` (run by
`make layout-check`) prints the C side's offsets against the same
numbers. Changing one file without the other fails one of those.
