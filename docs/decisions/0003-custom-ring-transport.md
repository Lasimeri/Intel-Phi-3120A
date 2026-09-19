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

## What was actually built (2026-09-19)

The decision held; three details of the implementation sketch did not.

- The card side is not a module. `phinet` was never written: the console
  has to work before a module could be loaded, and everything after it
  reuses the console's ring code, so the tty, the netdev, the rpc stream
  and the two block devices are kernel patches 0016, 0021, 0022, 0025 and
  0026. `card/drivers/phinet/` keeps only the shared C header.
- Doorbells were never plumbed. Both sides still poll, which the sketch
  allowed for bring-up and which turned out to be sufficient; interrupts
  remain the open item in phase P6.
- The channel list grew from two to five: console, network, rpc, block,
  host memory. The region grew with it, 1 MiB to 2 MiB to 16 MiB, and the
  block channel added a data area after its rings. None of that changed
  the header or ring layout the two sides agree on.

## Alternatives rejected

- Port `micvcons`/`micvnet`: Intel code, tied to 2.6.38 internals.
- virtio over shared memory: needs a doorbell/trap mechanism that plain
  memory does not provide; would have reinvented the ring anyway.
- NTB transport: elegant but twice the code (card hw driver plus host
  protocol reimplementation) for the same result.
