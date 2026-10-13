# 2026-10-13: 2 GiB of swap on every card beside the co-processor's window

The host-memory region (`/dev/phiblk1`, kernel patch 0026) was 2 GiB per
card on the rack and served either as the card's swap (the init's
`mkswap`/`swapon` at boot) or as the AVX-512 co-processor's window (its
worker turns the swap off at start: the window is the same bytes). The
user wants both on every card: "Phi 0 also needs 2GB's of SWAP, like the
rest of the cards" (all four had none while the workers ran).

## What changed

- `~/.config/phi/cards` on the rack: `HOSTMEM` 2G to 4G for all four
  cards (user configuration, outside the repository).
- `card/initramfs/init`: when `/dev/phiblk1` has 8388608 sectors or more
  (4 GiB), the swap goes on `/dev/loop0` set up over the device at offset
  2147483648 (`busybox losetup -o`), so it takes the half above 2 GiB and
  the co-processor's window keeps the first part (at most 768 MiB used,
  Intel-Phi-AVX512 `card/vpu/vpu_proto.md`). A smaller region is all
  swap, as before.
- The co-processor's start script looks for `phiblk1` in `/proc/swaps`
  and finds `/dev/loop0` instead, so it no longer turns the swap off.

## Measured (rack, four cards restarted with `phi-down.sh`, `phi-up.sh`)

| card | `/proc/swaps` | swap | `/dev/phiblk1` |
| --- | --- | --- | --- |
| phi, phi1, phi2, phi3 | `/dev/loop0` | 2048 MiB each, 0 used | 4096 MiB each |

The co-processor's worker then started on all four (`-S 12 -C 4`), each
card answering 24000 requests at 7 us median and 14 to 16 us p99 with the
swap on; `phitop` shows `swap on host RAM 0 KiB / 2047 MiB` on each.
The initramfs rebuilt by `card/initramfs/build.sh` (399 applets).
