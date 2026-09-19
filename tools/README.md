# tools/

Small C helpers compiled and run with `tcc`, used where a check has to see
a C header exactly as C sees it, or where a one-off image has to be built
byte by byte. They are not part of any build product and nothing links
against them.

| Tool | Purpose |
| --- | --- |
| `ring-layout-check.c` | Prints `sizeof`/`offsetof` for the ring transport structures in `card/drivers/phinet/include/phi_ring.h` and compares them with the values the Rust crate uses. Run by `make layout-check`, which `make check` includes. |
| `hlt-image.c` | Builds a stub boot image: the real bzImage's setup sectors followed by one sector that sets the card's boot flag in the ring header and halts. Used to separate a failing boot interrupt from a failing kernel. `hlt-image.md` has the commands. |

## What the layout check does and does not cover

`phi_ring.h` is a deliberately partial mirror. It pins the three
structures both sides must agree on byte for byte (`phi_region_hdr`,
`phi_channel_desc`, `phi_ring`) and stops there. It does not carry the
channel kinds added after P5 (3 rpc, 4 block, 5 host memory), the
host-memory fields the region header grew in kernel patch 0026, or any
ring access helper.

So a green `make layout-check` says the wire layout has not drifted; it
does not say the header is complete. The kernel's `asm/knc_ring.h` (patch
0021, extended by 0022, 0025 and 0026) is the complete C side, and
`host/crates/phi-ring/src/layout.rs` is the Rust side. Nothing compiles
against `phi_ring.h` except this check.

## Why C and not a script

`CLAUDE.md` forbids Python for tooling, and the point of
`ring-layout-check.c` is to ask a C compiler what C thinks the offsets
are; reimplementing the struct layout rules in another language would
defeat it. `tcc -run` compiles and runs in one step with no build
artifact.
