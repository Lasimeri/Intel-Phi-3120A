# phi-regs / postcode.rs

POST codes are the only progress signal available before the console ring
exists.

## Encoding (measured)

The register holds two ASCII characters in its low two bytes, low byte
first. Intel's table (MPSS 2.1 readme) lists codes as mixed-case strings
(`"0b"`, `"3d"`, `"dE"`), which only makes sense for characters, and the
first read on this card (2026-09-13) returned `0x6330`, the pair `"0c"`.
The earlier assumption that the low byte was a small integer was wrong and
is corrected here; `docs/results/2026-09-13-first-contact.md` has the raw
values.

## Open item

`"0c"` ("Cache C code") is an early bootstrap stage, yet `SPAD2` reported
the ready state at the same moment. Either the register is not updated
after the early stages on this flash version, or it is not the live POST
register. `phictl reset` traces the register at 10 ms resolution through
a reset to settle this; the answer is recorded in `docs/results/`.
