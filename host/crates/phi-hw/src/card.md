# phi-hw / card.rs

`Card` is the only type that touches registers. Every method maps to one
step of Intel's v5.9 driver, named in the doc comment, so that a reader can
diff the behavior against the reference.

## Facts the code depends on

- SBOX registers are 32-bit at `BAR4 + 0x10000 + offset`; the POST code is
  at `BAR4 + 0x242c` (`phi-regs::sbox`).
- Reset: `RGCR |= 1`, then at least one second of silence
  (`mic_x100_hw_reset`). Intel then cleared `SPAD2` and polled its bit 0
  (`mic_x100_reset_fw_ready`, `mic_x100_is_fw_ready`).
- Boot interrupt: write APIC ID to `APICICR7 + 4`, order it, write
  `229 | (1 << 13)` to `APICICR7` (`mic_x100_send_firmware_intr`). This code
  reads the register back after each write instead of relying on `wmb()`,
  because from userspace a read is the only way to know a posted PCIe write
  has arrived.
- Card memory writes are bounded by both the aperture size (16 GiB) and
  the GDDR size (6 GB), so a wrong address fails on the host instead of
  disappearing into unpopulated card address space.

## What is not here

DMA, interrupts from the card, and SMPT programming. They belong to phase
P6 and will live in sibling modules with the same shape.

## Testing

Only on hardware, through `phictl` with `PHI_BDF` set. The register
arithmetic it relies on is tested in `phi-regs`.

## Reset trace

`reset` samples the POST register every 10 ms from the `RGCR` write until
the ready flag returns and records each distinct value with its timestamp.
This is how the meaning of the register on this flash is established
(`phi-regs/src/postcode.md`, open item) and it costs nothing during a
normal reset.

## Paced aperture writes (2026-09-14)

`write_card_memory` issues posted writes in `APERTURE_WRITE_CHUNK`
(4 KiB) chunks and reads eight bytes back after each. The read is
non-posted: it cannot complete until the card has accepted every earlier
posted write, so at most one chunk is ever in flight. Measured: single
8-byte reads and writes at 64 MiB and 256 MiB are fine, an unpaced 1 MiB
burst of 8-byte writes reset the host with nothing logged (three times).
`write_card_memory_paced` exposes the chunk size and the read-back for
`phictl fill`.
