# card/drivers/phinet

One C header, `include/phi_ring.h`. No module is built here.

## Why the directory has this name

The plan (`docs/decisions/0003-custom-ring-transport.md`, written
2026-09-13) was an out-of-tree module named `phinet` carrying the card's
`tty` and `netdev` over the ring transport. That is not how it was built.
Both ended up in the kernel patch series instead, because the console has
to work before any module can load and because the netdev shares the
early console's ring code:

| What | Where it actually lives |
| --- | --- |
| `/dev/ttyPHI0`, the console tty | kernel patch 0016, `arch/x86/kernel/knc_tty.c` |
| `phi0`, the Ethernet device | kernel patch 0021, `arch/x86/kernel/knc_net.c` |
| `/dev/phirpc`, the agent's byte stream | kernel patch 0022, `arch/x86/kernel/knc_rpc.c` |
| `/dev/phiblk0`, `/dev/phiblk1`, `/dev/phihost` | kernel patches 0025 and 0026 |
| shared ring helpers used by all of them | `arch/x86/include/asm/knc_ring.h`, added by patch 0021 |

The directory and this name are kept because `include/phi_ring.h` is
referenced by path from `tools/ring-layout-check.c` and from
`host/crates/phi-ring/src/layout.md`. `docs/plan.md` records the same
deviation in the P4 row.

## include/phi_ring.h

The C mirror of the wire layout in `docs/spec/ring-protocol.md`:
`phi_region_hdr` (64 bytes), `phi_channel_desc` (64 bytes per channel),
`phi_ring` (192-byte header, head and tail each alone on a 64-byte line),
and the magic, version and channel-kind constants.
`_Static_assert`s pin the three sizes. `make layout-check` compiles
`tools/ring-layout-check.c` against it and compares every `offsetof` with
the constants in `host/crates/phi-ring/src/layout.rs`.

The header is a knowingly partial mirror: it stops at the structure
layout that both sides must agree on. It does not carry the channel kinds
added after P5 (3 rpc, 4 block, 5 host memory), the host-memory fields in
the region header, or any of the ring access helpers, because nothing
compiles against it except the layout check. The kernel's own copy,
`asm/knc_ring.h`, is the complete one.

## Barriers

KNC has no `sfence`. Patch 0003 makes `smp_wmb()` a
`lock addl $0,-4(%rsp)`, which is what the in-kernel ring code relies on.
Ring indices are 4-byte aligned `u32` stores, atomic on x86, and must be
read and written as single 32-bit accesses.
