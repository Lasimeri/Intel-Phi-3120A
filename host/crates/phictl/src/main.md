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

`console=phiring loglevel=8`. `phiring` is the name the KNC platform
code registers for the ring console driver (phase P3). The `memmap=` and
`phi.ring=` parameters are appended by `phi-hw::boot` unless
`--raw-cmdline`.

## Exit conditions

`Ctrl-C` stops the console. Errors from the layers below are printed as a
chain (`anyhow`), for example the "not bound to vfio-pci" check that runs
before any VFIO call so the message can name the fix.
