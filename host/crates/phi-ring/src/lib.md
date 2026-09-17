# phi-ring / lib.rs

Crate root for the ring transport. The design goal is that every line of
protocol logic runs in `cargo test` on a `Vec<u8>`, and the only thing the
hardware adds is a different `RingMemory` implementation (uncached BAR0
accesses, single 32-bit index accesses, plus a read-back fence:
`phi-hw/src/ringmem.rs`).

See `docs/spec/ring-protocol.md` for the wire layout and
`docs/decisions/0003-custom-ring-transport.md` for why this exists instead
of Intel's `micvcons`/`micvnet`.

## Module map

- `layout`: the offset constants and channel kinds.
- `memory`: the `RingMemory` trait and the test backend.
- `ring`: producer and consumer over one ring.
- `region`: formatting and opening a whole region with its channel table.

## Errors

`Error` is `PartialEq` so tests can compare it. `BadMagic` is shared by
the region header and the ring headers (the expected value tells them
apart); `BadLayout` is any header field that points outside the region or
breaks an alignment rule, reported by `Region::open` before the field is
used.

## Who uses it

`phictl` (console, network, rpc, block and host memory services) through
`Region::open` and `host_endpoints`; `phi-hw::boot` through
`Region::format` on a `VecMemory` image that is then copied to the card in
one bulk write.
