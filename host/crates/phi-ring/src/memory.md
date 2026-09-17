# phi-ring / memory.rs

`RingMemory` is the seam between protocol and hardware.

## Contract

- `len` bounds every offset; `Region::open` checks header fields against
  it so a mismatched region size is an error, not a panic in the backend.
- `read`/`write` copy bytes at region-relative offsets. They are not
  required to be atomic beyond what the backend does for aligned 4-byte
  values, which is why indices are written with `write_u32` at 4-aligned
  offsets: both KNC and the host CPU perform aligned 32-bit stores
  atomically, and PCIe carries them as single transactions. A backend on
  device memory must override `read_u32`/`write_u32` with single 32-bit
  accesses; the defaults (a 4-byte copy through `read`/`write`) are only
  right for plain memory. The aperture backend's byte copy would issue four
  byte writes, and the card observed torn indices that way on 2026-09-16
  (`phi-hw/src/ringmem.md`).
- `fence` is called by the producer between writing data and publishing
  `head`, and by the consumer between reading data and publishing `tail`.
  The card backend implements it as a read from the region, because a PCIe
  read cannot complete until earlier posted writes from the same CPU have
  reached the device (PCIe ordering rules, and the reason Intel's driver
  reads a register after writes in `mic_x100_send_firmware_intr`).

## Backends

`VecMemory` is the test backend and also serves to format a region image in
host memory that is then written to the card with one bulk copy, which is
faster than formatting through uncached byte writes.

`&mut M` implements the trait for any backend `M` and forwards every
method, the u32/u64 accessors included, so wrapping the aperture backend in
a reference cannot silently reintroduce byte-wise index writes. The test
`reference_backend_keeps_the_single_access_index_paths` pins this with a
counting backend reached through a generic parameter.
