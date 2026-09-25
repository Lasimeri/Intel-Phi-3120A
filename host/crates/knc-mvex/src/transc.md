# transc.rs: exp2 and the reciprocal, as single instructions

Knights Corner has a small set of transcendental approximations that
AVX-512 as a whole does not (the nearest, `vexp2ps` and `vrcp28ps`, are
AVX-512ER on Knights Landing, with other encodings and precisions). A
SwiGLU needs two of them, so they are here:

| function | instruction | encoding | accuracy | ISA reference 327364-001 |
| --- | --- | --- | --- | --- |
| `vexp223ps` | 2 to the power of each lane, read as int32 fixed point with 24 fraction bits | MVEX.512.66.0F38.W0 C8 /r | 0.99 ULP relative | page 190 |
| `vrcp23ps` | 1 / each float32 lane | MVEX.512.66.0F38.W0 CA /r | 0.912 ULP relative | page 577 |

Both take a register operand only. The `vexp223ps` page lists no valid
register swizzle but "no swizzle", and the `vrcp23ps` page says any
SwizzUpConv other than "no broadcast and no conversion" raises an
invalid-opcode exception, so the functions take a `Zmm` rather than a
`Src` and cannot be asked for an encoding the card would refuse.

`vexp223ps` does not take a float. Intel's exp2 is the pair
`vcvtfxpntps2dq` with exponent adjustment 24 (`conv.rs`,
`vcvtfxpntps2dq_adj` with `ExpAdj::Q8_24`, immediate 0x50 for round to
nearest) and then `vexp223ps`; the page states the pair's behaviour at
the ends: the conversion saturates, INT_MAX becomes +inf and INT_MIN
becomes +0, so no input can wrap. `kernelgen/glu.rs` is the one user.

The encodings are pinned by the tests here against the byte layout of
the row they share with `vpermd` (the 0F38 map, 66 prefix, no second
source), and checked on the card by `phi-vpu matmul-check`
(`check_swiglu`): the whole SiLU against the host's float64, worst
relative error 5.49e-7 for |g| up to 8 and 3.48e-6 up to the saturation
point (2026-09-23, card 0).
