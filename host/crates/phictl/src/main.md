# phictl / main.rs

The operator's tool. Subcommands map onto `phi-hw` one to one; the only
logic here is argument handling, printing, and the console loop.

| Command | What it does | Needs |
| --- | --- | --- |
| `info` | PCI identity, COMMAND register, POST code with description, all 16 scratchpads, decoded `SPAD2` | card bound to vfio-pci |
| `postcode [--watch] [--timeout S]` | POST code, optionally every change with a timestamp | same |
| `spad [N]` | one or all scratchpads | same |
| `regs` | named SBOX registers, non-zero SMPT entries, RDMASR0..7 | same |
| `reset [--timeout S]` | `RGCR` reset, wait for ready, print before/after | same |
| `boot --kernel P [--initrd P] [--cmdline S] [--ring-base X] [--ring-size X] [--raw-cmdline] [--no-console]` | the loader sequence, then tails the console | same, plus files |
| `console [--ring-base X] [--ring-size X]` | tail the card-to-host console ring and forward stdin lines to the host-to-card ring | a formatted ring region |

## Console loop

Polls the ring at 1 kHz when idle and as fast as data arrives otherwise;
also prints POST code changes and the card's boot-flags word so the very
first boot, where no console output may ever appear, still shows progress.
stdin is read on a helper thread and pushed line by line; if the ring is
full (card not consuming), the push retries with a short sleep.

## Default command line

`earlyprintk=phiring,keep loglevel=8`. `phiring` is the early console the
KNC kernel registers into the ring (card/kernel/patches, patch 0011); `keep`
leaves it registered after boot consoles are normally dropped, since no
other console exists until `phinet` provides a tty (phase P4, then
`console=` names that tty). The `memmap=` and `phi.ring=` parameters are
appended by `phi-hw::boot` unless `--raw-cmdline`.

## Exit conditions

`Ctrl-C` stops the console. Errors from the layers below are printed as a
chain (`anyhow`), for example the "not bound to vfio-pci" check that runs
before any VFIO call so the message can name the fix.

## `peek`

`phictl peek 0x4000000 --len 16` reads a few bytes of card memory through
the BAR0 aperture and prints them: one non-posted read, nothing else.
Added after the first `boot` attempt reset the host with no error logged
(2026-09-13); the loader no longer reads back what it wrote, and this
command exists so that aperture reads can be tested by themselves from
the physical console.

## `poke`

`phictl poke 0x4000000 0` writes one 8-byte value through the aperture,
the write-side twin of `peek`. Added 2026-09-14 after `boot --load-only`
reset the host during its first bulk write (the ring region, then at
32 MiB). The default ring base moved to 256 MiB at the same time.

## `fill`

`phictl fill 0x10000000 1048576` writes a megabyte of zeros through the
loader's own path: 4 KiB chunks, each followed by an 8-byte read-back
that waits for the card to accept the chunk. `--chunk` and
`--no-readback` reproduce other patterns. Added 2026-09-14 after a
1 MiB unpaced burst reset the host three times while single 8-byte
writes did not.

## Network bridge (`--net`)

`phictl boot --net phi0` and `phictl console --net phi0` run the TAP bridge
of `net.rs` on a second thread next to the console loop: the host gets a
`phi0` interface (address from `--net-addr`, default `10.9.0.1/24`) whose
frames travel through the ring region's network channel to the card's
`phi0` (kernel patch 0021). The card's init script assigns `10.9.0.2/24`
and starts dropbear, so `ssh root@10.9.0.2` works as soon as the banner
shows. See `net.md`.
