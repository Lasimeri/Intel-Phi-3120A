# phi-ring

Host side of the host/card shared-memory ring transport
(`docs/spec/ring-protocol.md`): the wire layout as offset constants, a
memory abstraction, single-producer/single-consumer byte rings, and the
region formatter and parser. Hardware-independent (`#![forbid(unsafe_code)]`);
the only dependency is `thiserror`. `phi-hw` supplies the aperture backend,
`phictl` the channel services. See `src/lib.md`.
