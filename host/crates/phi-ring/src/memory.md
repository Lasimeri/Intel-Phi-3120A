# phi-ring / memory.rs

`RingMemory` is the seam between protocol and hardware.

## Contract

- `read`/`write` copy bytes at region-relative offsets. They are not
  required to be atomic beyond what the backend does for aligned 4-byte
  values, which is why indices are written with `write_u32` at 4-aligned
  offsets: both KNC and the host CPU perform aligned 32-bit stores
  atomically, and PCIe carries them as single transactions.
- `fence` is called by the producer between writing data and publishing
  `head`, and by the consumer between reading data and publishing `tail`.
  The card backend implements it as a read-back of the ring magic, because
  a PCIe read cannot complete until earlier posted writes from the same
  CPU have reached the device (PCIe ordering rules, and the reason Intel's
  driver reads a register after writes in `mic_x100_send_firmware_intr`).

## Backends

`VecMemory` is the test backend and also serves to format a region image in
host memory that is then written to the card with one bulk copy, which is
faster than formatting through uncached byte writes.
