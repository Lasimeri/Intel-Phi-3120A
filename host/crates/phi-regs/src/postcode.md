# phi-regs / postcode.rs

POST codes are the only progress signal available before the console ring
exists. The bootstrap writes them during initialization; Intel's kernel kept
writing progress values after the handoff, and this project's kernel will do
the same (the KNC platform layer writes a value at each early-boot milestone,
which is how phase P3 tells "stuck in the decompressor" from "stuck in
`start_kernel`").

## Known values

Only the codes listed in the MPSS 2.1 readme are decoded. 0x12 is the one
that matters operationally: it means the bootstrap is idle and ready.

## Open item

The width of the meaningful field. Mainline reads a 32-bit register and
Intel's `micinfo` printed two hex digits. This module masks to the low byte
and the test documents that assumption; phase P1 records the full raw value
in `docs/results/` to confirm the upper bits are zero.
