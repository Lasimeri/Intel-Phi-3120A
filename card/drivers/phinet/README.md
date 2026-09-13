# card/drivers/phinet

Out-of-tree kernel module for the card (phase P4): the card side of
`docs/spec/ring-protocol.md`.

## Provides

- `/dev/ttyPHI0`: a `tty` over the console channel, so `getty` and the
  kernel console both work after boot (the early console in the kernel
  patch series covers the time before this module loads).
- `phi0`: a `netdev` over the network channel. Frames are `u16 length`
  plus payload in the ring; MTU 1500; no offloads. The host bridges the
  other end to a TAP device.

## Structure (planned)

| File | Role |
| --- | --- |
| `include/phi_ring.h` | Layout mirror of the Rust `phi-ring` crate, with `_Static_assert`s. Present now. |
| `phinet_ring.c` | Producer/consumer over `ioremap_cache`d card memory located by `phi.ring=` |
| `phinet_tty.c` | `tty_driver` with a poll kthread (v1) or doorbell interrupt (v2) |
| `phinet_net.c` | `net_device` with NAPI polling on the card-to-host ring |

## Barriers

KNC has no `sfence`; the kernel patch series makes `smp_wmb()` a
`lock addl $0,-4(%rsp)`, which is what this module relies on. Indices are
4-byte aligned `u32` stores, atomic on x86.
