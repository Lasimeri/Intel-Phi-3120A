# 2026-09-20: the integer half of the MVEX encoder, verified on the card

`knc-mvex` could encode float64 arithmetic and nothing else, which is why
every vector experiment so far has been a floating-point one. The integer
set is what a compression kernel needs, and it is now encoded and checked
on hardware.

## Added

Eleven instructions, from the opcode column of ISA reference 327364-001
chapter 6:

| Instruction | Encoding |
| --- | --- |
| `vpaddd` | MVEX.NDS.512.66.0F.W0 FE /r |
| `vpsubd` | MVEX.NDS.512.66.0F.W0 FA /r |
| `vpandd` | MVEX.NDS.512.66.0F.W0 DB /r |
| `vpandnd` | MVEX.NDS.512.66.0F.W0 DF /r |
| `vpord` | MVEX.NDS.512.66.0F.W0 EB /r |
| `vpxord` | MVEX.NDS.512.66.0F.W0 EF /r |
| `vpslld` | MVEX.NDD.512.66.0F.W0 72 /6 ib |
| `vpsrld` | MVEX.NDD.512.66.0F.W0 72 /2 ib |
| `vpsrad` | MVEX.NDD.512.66.0F.W0 72 /4 ib |
| `vpsllvd` | MVEX.NDS.512.66.0F38.W0 47 /r |
| `vpsrlvd` | MVEX.NDS.512.66.0F38.W0 45 /r |

That is the complete operator set FastLanes asks for
(`docs/research/compression-on-knc.md`), at the two lane widths this card
has. There are no byte or word integer vector instructions in the ISA at
all, so this set is not a subset of anything: it is the whole integer
vocabulary of the machine.

Two encoding shapes, not one. The bitwise and arithmetic forms are `NDS`
and differ from the existing `PD` forms in a single prefix bit (`W0`, not
`W1`). The immediate-count shifts are `NDD`: the destination is in `vvvv`,
the source in ModRM.r/m, and ModRM.reg carries the opcode extension, so
`vpslld`, `vpsrld` and `vpsrad` are three `/digit` values of one opcode.

## Verified

`card/examples/vpu_int.c` with `vpu_int.S`: one instruction per output
slot, sixteen slots, every lane compared against scalar C, two passes with
different operands.

```
0 of 32 checks failed
```

First run, nothing changed afterwards. The bytes are now pinned in
`integer_bytes_verified_on_the_card` in `host/crates/knc-mvex/src/lib.rs`.

This had to run on hardware. The float64 encodings reproduce Intel's k1om
kernel macros byte for byte, so a host unit test catches a mistake in
them; the integer encodings have no such reference. MVEX is dense enough
that a wrong prefix bit decodes as a different valid instruction rather
than faulting, and returns plausible numbers. `vpu_int.md` lists what each
slot is there to catch; the three that could not be cross-checked any
other way are the memory second source, the `NDD` form with a destination
above `zmm15` (which clears `V'`), and merge masking.

## Also pinned

`cargo test -p knc-mvex` now compares the generator's output against all
four committed `.S` files. `vpu_memcpy.S` had been generated and committed
on 2026-09-20 but never pinned, so an encoder change could have silently
diverged from the bytes the card was measured running.

## What it unblocks

The bit-unpacking measurement in `docs/results/2026-09-20-bitunpack.md`,
which is the step that decides whether any of this is worth continuing.
