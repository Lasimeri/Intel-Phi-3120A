# blkbench.c

A timer for the card's side of the host-memory block path: O_DIRECT
`pread` and `pwrite` of `/dev/phiblk1` at request sizes from 4 KiB to
16 MiB, a fixed total (16 MiB by default) at each size, reporting wall
time, mean and minimum per call and the resulting rate. The minimum per
call is the fixed cost of one request through the card's block layer,
the ring, the host service and the DMA engine; the 16 MiB row is the
link. The VPU worker's `pull` and `push` are the same calls, so this is
the transport it sees.

Reads start at offset 0; writes land at 4 GiB, above the worker's
data area (which starts at 1 MiB), so a run while a worker is idle
changes nothing the worker reads. Do not run it while a request is in
flight on the same card.

Built on the card: `phi -c N put card/vpu/blkbench.c /tmp/blkbench.c`,
`phi -c N run cc -O2 -o /tmp/blkbench /tmp/blkbench.c`,
`phi -c N run /tmp/blkbench`. Results: `docs/results/2026-09-22-block-pipeline.md`.
