# main.S: `phictl` in assembly

`phictl` is the host's control of the cards: it opens a card through
VFIO, boots it, and serves it (the console, the control socket, the
block devices, the SSH forward); its client verbs drive a served card.
Until 2026-09-29 it was the Rust crate `host/crates/phictl` over
`phi-vfio`, `phi-hw`, `phi-ring`, `phi-regs` and `phi-rpc`
(about 8000 lines); this directory is the same program in x86-64
assembly, no libc, GNU `as` and `ld` only. See the port's record,
`docs/results/2026-09-29-phictl-assembly.md`, for what was verified
against the Rust one and what changed.

| file | what |
| --- | --- |
| `main.S` | the entry, the argument walk, card selection, the register subcommands |
| `sysfs.S` | PCI addresses, sysfs lookups, the cards list |
| `vfio.S` | the container, group and device; region maps; config space; DMA mapping |
| `card.S` | registers, the bootstrap wait, reset, the boot interrupt, the aperture copies |
| `postcode.S` | POST codes as text and description |
| `ring.S` | the ring region: rings, opening, formatting |
| `boot.S` | `phictl boot`: the loader |
| `console.S` | the daemon loop and the console |
| `services.S` | the services' turns |
| `wire.S` | rpc frames: decoders, validation, building |
| `serve.S` | the control socket and the relay; sensors |
| `client.S` | exec, put, get, status, sensors, traffic |
| `dma.S` | the DMA engine |
| `disk.S` | the block services |
| `tcp.S` | the TCP/IP stack |
| `forward.S` | the SSH forward |
| `net.S` | the TAP bridge |
| `text.S` | the write cursor, output, strings, numbers, time, files |
| `defs.inc` | every constant |

**Arguments.** `--bdf ADDR` or `--card N` anywhere; `PHI_CARD` when
neither; `PHI_BDF` for the address when `--bdf` is absent (as the Rust
tool resolved them). Then a subcommand and its options; `usage` on
anything unknown, on standard error, exit 2. The register subcommands:

- `cards [--plain]`: the cards list, as the Rust tool printed it (byte-identical).
- `info`: identity, COMMAND, aperture size, POST code, SPAD2, SPAD4, the clock ratio, the sixteen scratchpads.
- `postcode [--watch] [--timeout S]`, `spad [N]`, `regs` (POSTCODE, RGCR, the interrupt registers, ICR7, SDBIC0, the non-zero SMPT entries, the RDMASRs), `reset [--timeout S]` (POST changes logged live on standard error, then the summary).
- `peek ADDR [--len N]`, `poke ADDR VALUE`, `fill ADDR LEN [--chunk N] [--no-readback]`: the aperture in isolation. A poke cannot be checked by a later peek from another process: each open resets the card and its memory.

Every subcommand that opens the card resets it (`vfio.md`), so they are
for a card that is down; on a running card the open is refused.

Calling convention throughout: System V (arguments in `rdi`, `rsi`,
`rdx`, `rcx`, `r8`, `r9`; `rbx`, `rbp`, `r12` to `r15` preserved),
except where a routine's comment says which registers it takes or
leaves. There is no heap: buffers are `.bss`, paged in as touched.
Multi-byte NOP padding is never emitted (`.p2align` only in data), so the
binary would pass `phi-isa-audit` were it ever run on a card; it is a
host program and uses AVX2 for the aperture copies.
