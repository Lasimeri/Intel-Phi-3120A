# More than one card

The stack runs up to 16 Xeon Phi cards in one host. A card is named by an
**index**, 0 to 15, and everything the host keeps for a running card is
derived from that index, so cards never share a socket, a host-memory
window, a port, a subnet or a hostname:

| resource | card 0 | card N |
| --- | --- | --- |
| control socket | `$XDG_RUNTIME_DIR/phictl/control.sock` | `$XDG_RUNTIME_DIR/phictl/N/control.sock` |
| host-memory window | `/dev/shm/phi-hostmem` | `/dev/shm/phi-hostmem-N` |
| SSH forward | `127.0.0.1:2222` | `127.0.0.1:2222+N` |
| ring subnet | host `10.9.0.1`, card `10.9.0.2` | `10.9.N.1`, `10.9.N.2` |
| hostname | `phi` | `phiN` |
| systemd unit | `phi@0.service` | `phi@N.service` |
| disk image, host memory | from `~/.config/phi/cards` | same file, line N |

Card 0 keeps every name the single-card stack used, so nothing written
against it changes. The derivation lives in one place on each side,
`host/crates/phi-vfio/src/cards.rs` and `scripts/phi-env.sh`, and
`phictl cards` is the only source of the index-to-address mapping.

## Which card is which

```
phi cards
2 card(s) known, order from /home/lasimeri/.config/phi/cards
card  bdf           present link         driver    group  up    hostmem  disk
0     0000:2f:00.0  yes     5.0 x8       vfio-pci  34     yes   6G       /mnt/1TB-NVMe/phi/disk.img
1     0000:24:00.0  yes     5.0 x4       vfio-pci  27     yes   6G       /mnt/1TB-NVMe/phi/disk1.img
```

Without `~/.config/phi/cards` the cards are ordered by PCI address. That
is fine for one card and wrong for more: on 2026-09-22 the second card,
on the chipset, enumerated at `24:00.0`, below the original at
`2f:00.0`, and the service that boots "the card" booted the new one with
the original's disk image. `phi cards init [DIR]` writes the file from
the bus in address order with a disk image path per card; edit the line
order to pin the identities. A card listed but not on the bus keeps its
index and is reported absent, so pulling card 1 does not renumber card 2.

## Bringing a new card up

1. Seat it, and connect **both** auxiliary power connectors: a 300 W SKU
   refuses to power up, and therefore to train its link, with either one
   missing (datasheet 328209, "Supplemental Power Connectors"). A card
   that did not power up leaves no trace in `lspci` and does not affect
   POST.
2. Reboot. The modprobe option `ids=8086:225d` binds every card to
   `vfio-pci`, and the udev rule gives every VFIO group to group `phi`.
   `scripts/verify-card.sh` prints all of them.
3. `phi cards init` if the file does not exist yet, else add a line.
4. A disk image: `scripts/phi-disk.sh create /mnt/1TB-NVMe/phi/diskN.img 64G`,
   named on the card's line.
5. `scripts/phi-autoboot.sh install N` (or `install all`) enables
   `phi@N.service`; `phi -c N status` shows it up.
6. The native toolchain lives on each card's own disk:
   `PHICTL_SOCKET=$XDG_RUNTIME_DIR/phictl/N/control.sock bash card/userland/components/clang-push.sh`
   once per image.

## Using a card

Every tool takes the index: `phi -c N ...`, `phictl --card N ...`,
`phitop -c N`, `phi-vpu --card N`, `scripts/phi-vpu.sh -c N ...`, and
`PHI_CARD=N` in the environment does the same for all of them. `phi -c N
sh` reaches the card over its own forward; the cards all boot the same
image and present the same SSH host key, so the one pinned entry in
`~/.ssh/known_hosts_phi` covers every port through `HostKeyAlias=phi`.
For a `~/.ssh/config` stanza per card, copy the `Host phi` one with
`Port 2222+N` and `HostKeyAlias phi`.

`phitop` with no card named shows every card in its own block, sampled
on its own socket from its own model; `n` and `p` focus one card, `a`
returns to all.

## Limits

- 16 cards: the subnet `10.9.N.0/24` and the port `2222+N` schemes, one
  constant (`MAX_CARDS`). A seventeenth is refused with a message.
- Host memory is per card: 6 GiB each by default, pinned. Sixteen cards
  would pin 96 GiB; set the size per line in `~/.config/phi/cards`.
- Each daemon holds its own VFIO group and IOMMU domain, so the fixed
  host-memory IOVA the card driver expects is fine on every card.
- Link width is whatever the slot gives. On this host card 0 is Gen2 x8
  on a CPU slot and card 1 is Gen2 x4 on the chipset, which halves its
  bulk DMA rate against card 0; compute is unaffected.
