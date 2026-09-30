# host/asm

The host programs of this stack in x86-64 assembly (GNU `as`, AT&T
syntax), built with binutils alone: no cargo, no libc.

| directory | what | replaced |
| --- | --- | --- |
| `phictl/` | `phictl`: opens a card through VFIO, boots it, serves it (console, control socket, disk and host memory over the DMA engine, the SSH forward on its own TCP stack, the TAP bridge), and the client verbs | the Rust `phictl` crate and its libraries `phi-vfio`, `phi-hw`, `phi-ring`, `phi-regs` (2026-09-29) |

`make build` builds it (`phictl/build.sh`) and installs the binary at
`host/target/debug/phictl`, the path every script of the family
resolves (`CONTRIBUTING.md`, "The family"), so nothing that consumed
the Rust binary changes.

What stays Rust in `host/crates/`: `phi-rpc` (the wire's reference
implementation and the test harness the card agent is checked against),
`phi-isa-audit` (a front end over the iced-x86 decoder; rewriting an
x86 decoder is a project of its own), `knc-mvex` (the MVEX generator,
carried byte for byte by Intel-Phi-AVX512), and `phitop` until it is
ported.

Why assembly, and what it bought: `phictl/main.md` and
`docs/results/2026-09-29-phictl-assembly.md`. The aperture write path
is 6.5x to 9x faster (32-byte stores), the SSH forward's upload about
6x, everything else the same speed, since the rings are bound by the
card's polling; one thread and no lock instead of five threads.
