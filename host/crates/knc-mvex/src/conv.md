# conv.rs (knc-mvex)

Why this exists: the card's vector unit has 32-bit lanes and no byte or
word arithmetic, but its memory operands can be read through an
up-conversion (ISA reference, tables 2.9 and 2.10, the `SSS` field of the
MVEX prefix): sixteen bytes become sixteen floats, or sixteen integers,
as they load, and one float becomes sixteen. That is what makes the
quantized weight formats of llama.cpp (4-, 5-, 6- and 8-bit integers
under a float16 scale) workable on the card: the bytes load as floats,
the nibbles are split with a multiply, a floor and a fused subtract, the
scales apply as broadcast operands. lib.rs encodes the plain forms only;
this file adds the conversions and the handful of instructions the
quantized kernels need on top.

| item | what |
| --- | --- |
| `Conv` | the SSS values: `Bcast1` (`{1to16}`), `Bcast4`, `F16`, `U8`, `S8`, `U16`, `S16`; the values are Intel's `_MM_UPCONV_*` enumerations, and `F16` reproduces the float16 load lib.rs already had |
| `Src::MemConv` | a memory source with a conversion, accepted by every three-operand instruction of lib.rs (`arith`, `arith_ps`) and by the forms here |
| `vmovaps_load_conv`, `vmovdqa32_load_conv` | aligned loads through a conversion, float32 and int32 |
| `vloadunpacklps_conv`, `vloadunpackhps_conv` | the unaligned pair into float32 lanes through a conversion (D1 / D5; a converted source is aligned only to its element size, so sixteen bytes load from any address) |
| `vloadunpackld_conv`, `vloadunpackhd_conv` | the same into int32 lanes (D0 / D4): the bytes arrive as integers |
| `vcvtfxpntdq2ps` | int32 to float32 (MVEX.512.0F3A.W0 CB ib, no legacy prefix: the encoding the rewriter uses for `vcvtdq2ps`) |
| `vrndfxpntps`, `Round` | float32 to an integral value, mode in the immediate (MVEX.512.66.0F3A.W0 52 ib: the rewriter's `vrndscaleps`) |
| `vfnmadd231ps`, `vfmsub213ps`, `vfmsub231ps` | the FMA family continued, opcodes as in AVX-512 (BC, AA, BA) |
| `vprefetch`, `Cache` | `vprefetch0` (L1) and `vprefetch1` (L2), MVEX.512.0F.W0 18 /1 and /2: the software prefetch the kernels stream their weights with |
| `vpermd` | lane permute by index, the sixteen-entry table lookup the iq4_xs format needs (MVEX.NDS.512.66.0F38.W0 36) |
| `vcvtfxpntps2dq` | float32 to int32 with a rounding mode (MVEX.512.66.0F3A.W0 CB ib, the rewriter's `vcvtps2dq`) |
| `vcvtfxpntps2dq_adj`, `ExpAdj` | the same with an exponent adjustment in I6..I4 of the immediate, so the integer is fixed point with that many fraction bits (ISA reference 327364-001, page 169); `Q8_24` feeds `vexp223ps` (transc.rs) |
| `vmovaps_store_conv` | store through a down-conversion under a write-mask (MVEX.512.0F.W0 29 with SSS the `Df32` value, page 379 and table 2.12): `Conv::F16` writes sixteen halves, 32 bytes, and a lane whose mask bit is clear is not written, which is how two threads store their own lanes of one vector (Intel-Phi-AVX512's kernelgen/glu.rs) |

Sources for the encodings: the two conversion opcodes and the `{1to16}`
field are the ones Intel-Phi-AVX512's `host/crates/avx512-xlate/src/rewrite.rs` emits for
the seamless path, which its `tools/avx512-narrow-test.c` checked lane for
lane against hardware semantics (its `docs/results/2026-09-22-full-avx512.md`);
the conversion values follow the float16 load lib.rs had (SSS 011, in
use since the ggml backend's first kernels) and Intel's enumeration
order. The tests pin the bytes; the kernels built on them are checked
against a host reference by `phi-vpu matmul-check` before use.

The exponent adjustment and the converting store were added for the
fused feed-forward's SwiGLU (2026-09-23). Their bytes are pinned by the
tests here (a converting store is the plain store with SSS set, and no
adjustment is the plain conversion byte for byte), and the SwiGLU built
on them is checked on the card lane by lane, in float32 and float16,
including ranges whose partial vectors are masked
(Intel-Phi-AVX512's `host/crates/phi-vpu/src/matmul.rs`, `check_swiglu`).
