# cards.rs: which card is which, and what each one owns

A host can run up to 16 Xeon Phi cards with this stack. A card is named by
an index, and every host-side resource a running card needs is a pure
function of that index:

| resource | card 0 | card N |
| --- | --- | --- |
| control socket | `<runtime>/phictl/control.sock` | `<runtime>/phictl/N/control.sock` |
| host-memory window | `/dev/shm/phi-hostmem` | `/dev/shm/phi-hostmem-N` |
| SSH forward | `127.0.0.1:2222` | `127.0.0.1:2222+N` |
| ring subnet | `10.9.0.1` host, `10.9.0.2` card | `10.9.N.1`, `10.9.N.2` |
| hostname | `phi` | `phiN` |
| systemd unit | `phi@0.service` | `phi@N.service` |

Card 0 keeps the names the single-card stack used. The limit of 16 comes
from the subnet and port schemes and is one constant, `MAX_CARDS`; a
seventeenth card is refused with a message rather than colliding.

## Why the order is a file and not the bus

Sorted PCI addresses were the order until 2026-09-22. Adding a second card
on the chipset put it at `24:00.0`, below the original at `2f:00.0`, and
the service that boots "the card" booted the new one with the old one's
disk image while the original sat idle. So the order lives in
`~/.config/phi/cards` (`$XDG_CONFIG_HOME` respected), one card per line,
the line order being the index, with an optional disk image and
host-memory size per line:

```
# BDF            DISK                          HOSTMEM
0000:2f:00.0     /mnt/1TB-NVMe/phi/disk.img    6G
0000:24:00.0     /mnt/1TB-NVMe/phi/disk1.img   6G
```

Without the file the enumerated cards are used in address order, which is
right for one card and a guess for more. A listed card that is not on the
bus keeps its index and is reported absent, so pulling card 1 does not
turn card 2 into card 1.

## Sizing HOSTMEM

The column is how much host memory the card is given (`/dev/shm/phi-hostmem`
for card 0, `phi-hostmem-N` for the others), 6G by default. It is the
card's swap device, the staging area for the block path, and the window
the AVX-512 co-processor's worker talks through. It is **pinned, shared
memory on the host**, so it is not available to anything else, and on a
host where the working set is large the size is a real choice rather
than a default to leave alone: two 6 GiB windows on a 31 GiB host left
less page cache than a 17.6 GB language model needed, and the host read
weights from the NVMe while it worked (prompt processing 6.22 against
9.33 tokens per second, the same binary and model, measured both ways;
`Intel-Phi-AVX512`, `docs/results/2026-09-23-ceilings-and-residency.md`).

What each user needs: the co-processor's matrix-multiply service uses
768 MiB of it, the seamless path a good deal less, and the card's swap
only what the card actually swaps, which is nothing when `phi vpu` has
turned swap off. 2G a card is enough for all of that; 6G is right when
the card is meant to swap into host memory.

`PHI_CARD=N` selects a card for every tool (`phictl`, `phitop`, `phi-vpu`,
and through `scripts/phi-env.sh` every script); `phictl --card N` and
`phi -c N` are the same thing on the command line. `phictl cards` prints
the list with each card's state.
