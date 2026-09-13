# phi-vfio / mapping.rs

A BAR mapping with volatile accessors and bounds checks.

## Why volatile, and why these widths

- The kernel maps VFIO regions uncached. Plain Rust loads and stores could
  be merged, reordered, or optimized away; `read_volatile`/`write_volatile`
  prevent that. They do not order accesses relative to each other across
  PCIe; callers that need a posted write to have landed (before sending an
  interrupt to the card) read a register back, as Intel's driver did with
  `wmb()` plus the read in `mic_x100_send_firmware_intr`.
- SBOX registers are 32-bit. `read32`/`write32` assert 4-byte alignment
  because an unaligned MMIO access splits into transactions the register
  block may not accept.
- The aperture (BAR0) is ordinary memory behind a BAR: `write_bytes` uses
  8-byte stores for throughput and byte stores for the ragged ends. This is
  how the 40 MB boot image gets into card memory.

## Not a general-purpose abstraction

There is no `Deref` to a slice on purpose: a slice would let the compiler
read device memory with ordinary loads, and Rust's memory model does not
cover MMIO. All access goes through the methods here.

## Testing

`memfd` stands in for a BAR so the alignment paths of `write_bytes` and
`read_bytes` are exercised on real memory without the card. An out-of-bounds
access is checked to panic rather than fault.
