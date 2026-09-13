# phi-hw / lib.rs

Crate root. Composition only: `card` (the device), `boot` (the loader
sequence), `ringmem` (the aperture as a `RingMemory` backend for
`phi-ring`).

The error type wraps the layers below so that `phictl` prints one chain,
e.g. `bootstrap not ready: POST code 0x0e, SPAD2 0x04000340 (reset the card
first)`.
