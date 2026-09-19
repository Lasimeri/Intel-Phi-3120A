# memhog.c

Allocates and touches a given number of MiB so the card's memory and its
host-RAM swap can be exercised deliberately.

```
cc -O2 -o memhog memhog.c
./memhog MIB [--verify] [--hold SECONDS]
```

Why it exists: `/tmp` on the card is a tmpfs, and tmpfs is capped at half of
RAM, so filling it stops at about 2.8 GiB and never reaches swap (measured
2026-09-19, the first attempt at testing the 6 GiB swap). Anonymous memory
has no such cap. One store per 4 KiB page makes the page resident; the
kernel then evicts older pages to `/dev/phiblk1`, which is host RAM served
over the ring (kernel patch 0026). `--verify` reads every page back and
compares it against a value derived from the page index, which is what
forces the evicted pages to come back over PCIe and proves they came back
intact.

Exit status 1 on an allocation failure or any mismatch, so it is usable as a
check rather than only as a demonstration.

## Measured, 2026-09-19

Card with 5669 MiB of usable GDDR5 and 6143 MiB of swap on host RAM,
`./memhog 8192 --verify`:

| Phase | Result |
| --- | --- |
| touch, first 5 GiB | about 490 MB/s, all resident |
| touch, remainder | about 170 MB/s once eviction starts |
| touch, whole 8 GiB | 29.7 s, 289 MB/s average |
| verify, whole 8 GiB | 74.9 s, 115 MB/s, **0 mismatches** |

The verify pass is slower than the touch pass because it reads pages back
that the touch pass pushed out: every one of those is a swap-in over the
ring, interleaved with resident pages that cost nothing. The full record is
`docs/results/2026-09-19-card-os.md`.

## Arguments

`MIB` must be a nonzero multiple of 256, which is the chunk size: one
`malloc` per chunk rather than one huge allocation, so a partial failure
reports how far it got instead of failing at zero. `--hold SECONDS` keeps
the memory resident so `phitop` or `free` on the card can be watched while
it is held.
