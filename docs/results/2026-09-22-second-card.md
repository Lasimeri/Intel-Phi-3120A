# A second card, and a stack that addresses sixteen

2026-09-22. Host: Ryzen 7 5800X on an MSI MEG X570 GODLIKE (MS-7C34,
BIOS 1.E0), Linux 7.2.6-1-cachyos. A second Xeon Phi 3120-series card
was installed in the chipset x16 slot (PCI_E4, X570 x4). Commit: this
record's.

## It did not enumerate, and the software could not have known why

After the first reboot with the card installed, `lspci` showed one Phi,
the topology was byte for byte the previous boot's (56 PCI functions,
diff empty), the CPU slot pair was still x8/x0/x8 and every chipset port
had its device. AMD firmware hides a root port whose link did not train,
so a card with no link is indistinguishable from an empty slot, and the
host POSTs regardless.

The cause was mechanical: the card was not fully seated, so its link
never trained. (Intel's datasheet, 328209 "Supplemental Power
Connectors", gives a second way to the same symptom: a 300 W SKU does
not power up unless both auxiliary supplies are detected. Software sees
the two identically.) Reseated, the card appeared at once:

```
24:00.0 Co-processor [0b40]: Intel Corporation Xeon Phi coprocessor 3120 series [8086:225d] (rev 20)
```

on chipset downstream port `21:02.0` at Gen2 x4, bound to `vfio-pci` by
the `ids=8086:225d` modprobe option with nothing typed, IOMMU group 27.
The two rebooted layouts also moved the first card's root port (the
3090 Ti went to x16 while the 3120A was unseated, then back to x8/x8),
which is why `scripts/verify-card.sh` now reports every card rather
than the first.

## It is a different SKU, and the stack did not care

| | card 0 | card 1 |
| --- | --- | --- |
| subsystem | `8086:3c98` (3120A) | `8086:3608` |
| BAR0 | 16 GiB | 8 GiB |
| link | Gen2 x8, CPU | Gen2 x4, chipset |
| core voltage | 1100 mV | 960 mV |
| kernel, image, CPUs, memory | the same: 7.2.3+28 patches, 228 CPUs, 5669 MiB | the same |

The aperture code sizes itself from BAR0, so the 8 GiB card booted the
first time it was tried, over the x4 link, with the same kernel and
image: bootstrap POST to "12" in 9 s, 228 CPUs online, host memory
served as swap, `/data` mounted.

## What changed so that two cards are two cards

The single-card stack booted "the first 8086:225d device", which after
the second card was the new one: it came up with the original card's
disk image while the original sat idle. Identity now comes from an
index, 0 to 15, pinned in `~/.config/phi/cards`, and every host-side
resource is a function of it (`docs/howto/multiple-cards.md`):

| resource | card 0 | card N |
| --- | --- | --- |
| control socket | `phictl/control.sock` | `phictl/N/control.sock` |
| host-memory window | `/dev/shm/phi-hostmem` | `/dev/shm/phi-hostmem-N` |
| SSH forward | 2222 | 2222 + N |
| ring subnet, hostname | `10.9.0.0/24`, `phi` | `10.9.N.0/24`, `phiN` |
| unit | `phi@0.service` | `phi@N.service` |

`phi.service` became the template `phi@.service`, whose instance runs
`scripts/phi-boot.sh -c N`, the same command line `phi-up.sh` runs, so a
card booted by hand and one booted at host boot are the same card. The
card's own init takes its address and hostname from the kernel command
line (`PHI_CARD_ADDR`, `PHI_HOSTNAME`), which the kernel hands to init
as environment variables, so the card side needed one `if`.

Verified, both cards up under `phi@0` and `phi@1`:

```
phi -c 1 sh 'hostname; ip -4 -o addr show phi0'
phi1
2: phi0    inet 10.9.1.2/24 scope global phi0
```

with card 0 still answering as `phi` at `10.9.0.2` on port 2222, its
legacy socket and window paths unchanged, and `ssh phi` (the old
`~/.ssh/config` stanza) still landing on card 0.

## Both cards as AVX-512 co-processors at once

`scripts/phi-vpu.sh -c N deploy`, `start`, then the host driver on both
cards concurrently, 1048576 elements each, and card 1 alone at 16 M:

| | pull | compute | push | GFLOP/s | lanes |
| --- | --- | --- | --- | --- | --- |
| card 0, 1 M, concurrent with card 1 | 6.4 to 22 ms | 0.292 ms | 1.7 ms | 216 | all bit-identical |
| card 1, 1 M, concurrent with card 0 | 10.3 to 11.9 ms | 0.688 ms | 3.8 ms | 92 | all bit-identical |
| card 1, 16 M, alone | 61 ms | 2.694 ms | 53 ms | **374** | all bit-identical |

Every lane on both cards matched the host's FMA3 hardware. Two things
in the numbers:

- The chipset card moves data at roughly half the CPU card's rate
  (16 MiB in 61 ms against 43 ms, out in 53 against 23), as an x4 link
  should. Compute is the vector units' and does not see the link.
- Card 1's compute at 16 M is 374 GFLOP/s against card 0's 316. Same
  clock, same thread count, same kernel; the 3608 board runs its core at
  960 mV where the 3120A runs 1100 mV. Not investigated further here.

The 1 M concurrent rows show what the per-card windows and sockets
bought: two daemons, two DMA engines, two host-memory files, no
interference between them beyond the host's own PCIe root.

## phitop shows each card on its own

`phitop` with no card named lists every card in `phictl cards` and
samples each on its own socket into its own model; the frame gives every
card a block with its header, temperatures, memory, grid and top
processes, and a card that is down says so while the others keep
updating. `n` and `p` focus one card, `a` returns to all. Recorded with
`phitop --batch 1` while both idled: card 0 at 61 C, card 1 at 62 C,
each with its own process table.

## Three things that cost time

1. A script created fresh has no execute bit: `phi@N` failed with
   `Permission denied` at `EXEC` on `phi-boot.sh` until `chmod +x`.
2. The card option parser read the whole command line, so
   `phi -c 1 run sh -c '...'` handed the shell's command to the index
   check. Only leading options are parsed now.
3. A non-login shell over the card's SSH forward has no `/opt/phi/bin`
   on `PATH` until the next boot links the toolchain into `/usr/bin`;
   the deploy step names the path, and `$PATH` in that ssh line had to
   be escaped or the host's `PATH` went across.
