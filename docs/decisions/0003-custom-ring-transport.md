# 0003: Console and network use a new ring protocol

Status: accepted, 2026-09-13.

## Context

Intel's card-side console (`micvcons`), network (`micvnet`), and SCIF all
live in the 2.6.38 tree and speak wire formats defined by `mpss-modules`.
Porting them to a mainline kernel means porting Intel code; the goal is to
not port MPSS. Mainline's NTB transport was considered: it would need an
`ntb_hw` driver on the card plus a userspace reimplementation of the
`ntb_transport` handshake on the host.

## Decision

`docs/spec/ring-protocol.md`: one region of card GDDR reserved by the kernel
command line holds a header and paired single-producer/single-consumer byte
rings per channel (console, network). The host accesses it through BAR0; the
card maps it cacheable. Doorbells: host to card via SBOX ICR, card to host via
RDMASR/MSI-X; both sides also poll, so the first bring-up needs no interrupts.

Host side: `phi-ring` (Rust, hardware-independent, unit-tested). Card side:
`phinet` (C kernel module, ~600 lines target) providing a `tty` and a
`netdev`; plus an early console writer of a few dozen lines.

## Consequences

- No SCIF, no COI, no offload: SSH plus native execution is the model.
- First console output arrives before any interrupt plumbing exists.
- Both sides share one `#[repr(C)]`/`struct` layout, cross-checked by a
  `tcc`-compiled helper.

## Alternatives rejected

- Port `micvcons`/`micvnet`: Intel code, tied to 2.6.38 internals.
- virtio over shared memory: needs a doorbell/trap mechanism that plain
  memory does not provide; would have reinvented the ring anyway.
- NTB transport: elegant but twice the code (card hw driver plus host
  protocol reimplementation) for the same result.
