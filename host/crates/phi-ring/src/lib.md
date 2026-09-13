# phi-ring / lib.rs

Crate root for the ring transport. The design goal is that every line of
protocol logic runs in `cargo test` on a `Vec<u8>`, and the only thing the
hardware adds is a different `RingMemory` implementation (uncached BAR0
writes plus a read-back fence).

See `docs/spec/ring-protocol.md` for the wire layout and
`docs/decisions/0003-custom-ring-transport.md` for why this exists instead
of Intel's `micvcons`/`micvnet`.

## Module map

- `layout`: the `#[repr(C)]` structures and constants.
- `memory`: the `RingMemory` trait and the test backend.
- `ring`: producer and consumer over one ring.
- `region`: formatting and opening a whole region with its channel table.
