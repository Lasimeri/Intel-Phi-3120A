# host/asm

The host programs of this stack in x86-64 assembly (GNU `as`, AT&T
syntax), built with binutils alone: no cargo, no libc.

| directory | what | replaced |
| --- | --- | --- |
| `phictl/` | `phictl`: opens a card through VFIO, boots it, serves it (console, control socket, disk and host memory over the DMA engine, the SSH forward on its own TCP stack, the TAP bridge), and the client verbs | the Rust `phictl` crate and its libraries `phi-vfio`, `phi-hw`, `phi-ring`, `phi-regs` (2026-09-29) |
| `phitop/` | `phitop`: the live viewer of every card (load grid, temperatures, memory, PCIe and device rates, processes), sharing `text.S`, `sysfs.S` and `wire.S` with phictl | the Rust `phitop` crate (2026-09-29) |

`make build` builds both (`phictl/build.sh`, `phitop/build.sh`) and installs
them at `host/target/debug/phictl` and `host/target/debug/phitop`, the paths
every script of the family resolves (`CONTRIBUTING.md`, "The family"),
so nothing that consumed the Rust binaries changes.

What stays Rust in `host/crates/`: `phi-rpc` (the wire's reference
implementation and the test harness the card agent is checked against),
`phi-isa-audit` (a front end over the iced-x86 decoder; rewriting an
x86 decoder is a project of its own), `knc-mvex` (the MVEX generator,
carried byte for byte by Intel-Phi-AVX512). The Rust sources these
directories' docs name as what they were ported from are in the history
at commit 230a124.

Why assembly, and what it bought: `phictl/main.md` and
`docs/results/2026-09-29-phictl-assembly.md`. The aperture write path
is 6.5x to 9x faster (32-byte stores), the SSH forward's upload about
6x, everything else the same speed, since the rings are bound by the
card's polling; one thread and no lock instead of five threads.
