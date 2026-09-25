# lib.rs (knc-mvex)

Why this exists: the card's 512-bit vector unit uses the MVEX prefix, a
four-byte prefix starting with 62H that only Intel's dead k1om toolchain
and ICC ever emitted (ISA reference 327364-001, chapter 3). LLVM's x86
backend, the card's own clang and every assembler in this stack reject or
misdecode it, so vector code is written as `.byte` sequences, and this
crate produces those bytes.

Prefix layout (section 3.3, and Intel's `mic_ni.h` macros in the k1om
kernel, which the tests reproduce byte for byte):

| byte | bits | meaning |
| --- | --- | --- |
| 62H | | MVEX escape |
| P0 | `R X B R' 0 0 m m` | register extensions, stored inverted (R and R' extend ModRM.reg, X and B extend r/m); `mm` = 01 for 0F, 10 for 0F38, 11 for 0F3A |
| P1 | `W v v v v 0 p p` | `vvvv` = first source register, inverted; bit 2 is 0 (EVEX has 1 here); `pp` = 00 none, 01 66H |
| P2 | `E S S S V' a a a` | eviction hint, swizzle/conversion, `vvvv` bit 4 inverted, write mask `k` |

Then the opcode, ModRM, an optional disp32 and an optional imm8. A vector
register in ModRM.reg is extended by R and R', one in ModRM.r/m by X and
B. Only the plain forms are produced: `E = 0`, `SSS = 000` (no swizzle,
no conversion, round to nearest), memory operands as `[base + disp32]`
with mod = 10 so the disp8*N compression never applies. `Mem::new`
refuses `rsp` and `r12` as a base (they need a SIB byte, which the
encoder does not emit); `rbp` and `r13` are fine, since only mod = 00
with r/m = 101 is RIP-relative. Mask register moves and `kortest` are
two-byte VEX instructions (`C5 F8 opcode ModRM`), limited to the first
eight general registers.

Instructions covered:

| family | forms |
| --- | --- |
| move | `vmovaps` load/store (the form Intel's kernel uses), `vmovapd` load/store/move |
| float64 | `vaddpd`, `vsubpd`, `vmulpd`, `vfmadd213pd`, `vfmadd231pd`, `vcmppd` (its write mask acts as an AND on the result: a clear mask bit clears the result bit, table 6.3 and the note under it) |
| int32 | `vpaddd`, `vpsubd`, `vpandd`, `vpandnd` (note the order: `(!zmm2) & src`), `vpord`, `vpxord`, `vpslld`, `vpsrld`, `vpsrad`, `vpsllvd`, `vpsrlvd` |
| mask | `kmov` in all three directions, `kortest` |
| conversions and more (`conv.rs`, `conv.md`) | memory operands through the up-conversions and broadcasts (`Src::MemConv`), the int32 and float32 unpack pairs with conversions, `vcvtfxpntdq2ps`, `vcvtfxpntps2dq` and its exponent-adjusted form, `vrndfxpntps`, `vfnmadd231ps`, `vfmsub213ps`, `vfmsub231ps`, `vpermd`, `vprefetch0/1`, the store through a down-conversion |
| transcendentals (`transc.rs`, `transc.md`) | `vexp223ps` and `vrcp23ps`, the SwiGLU's exp2 and reciprocal |

Every integer vector instruction on this machine operates on 32-bit or
64-bit lanes; there are no byte or word forms in the ISA at all, so the
`D` set above is the complete integer vocabulary a codec can use here
(`docs/research/compression-on-knc.md`). The `D` forms are `W0` where the
`PD` forms are `W1`, which is the only difference in the prefix.

The immediate-count shifts are the one `NDD` family: the destination is
in `vvvv`, the source in ModRM.r/m, and ModRM.reg carries the opcode
extension of opcode 72 (`/6` left, `/2` logical right, `/4` arithmetic
right), so `vpslld`, `vpsrld` and `vpsrad` share one encoder path and one
opcode byte. Everything else here is `NDS`: destination in ModRM.reg,
first source in `vvvv`, second source in r/m.

Each function returns an `Insn` with the bytes and the Intel-syntax text
(`[rbp-64]` for a negative displacement); `gas()` renders a `.byte` line
with the text as a comment, `c_string()` a C string literal for inline
assembly.

## Tests

Three sources pin the bytes, in decreasing strength:

1. Hardware: `probe_bytes_verified_on_the_card` holds the bytes of
   `card/examples/vpu_probe.S`, which ran on the card on 2026-09-15 with
   every lane checked (`docs/results/2026-09-15-vpu.md`), and
   `integer_bytes_verified_on_the_card` holds the bytes of
   `card/examples/vpu_int.S`, which ran on 2026-09-20 with 0 of 32 checks
   failing (`docs/results/2026-09-20-mvex-integer.md`). The integer set
   has no Intel macro to reproduce, so hardware is its only reference,
   which is why the probe covers a memory second source, the `NDD` form
   with a register above `zmm15`, and merge masking.
2. Intel's macros: the `vmovaps` load and store for all 32 registers and
   the `kmov` r32 forms for all 8 masks reproduce `mic_ni.h` byte for byte.
3. The document: the extension bits for registers 8 to 31, memory bases
   above `rdi`, `kmov k, k` and `vfmadd231pd` (the one form no generated
   file uses) follow section 3.3 and the opcode column of chapter 6, and
   have not run on the card.

The generator's tests (`main.md`) compare its output with the committed
files, so an encoder change is visible until they are regenerated.

## The float32 and unaligned-store forms (2026-09-21)

Added for the AVX-512 translator (`host/crates/avx512-xlate`), which needed
the float32 side of the arithmetic the crate already had in float64.

`vaddps`, `vsubps`, `vmulps` and `vcmpps` differ from their float64
counterparts in exactly the two bits that select the data type: no legacy
prefix instead of `66`, and `W0` instead of `W1`. The opcodes are the same.
EVEX makes the same distinction with the same two fields, which is a large
part of why the translation is mechanical.

`vfmadd213ps` and `vfmadd231ps` are the exception and it is easy to get
wrong: they are in the `0F38` map and *do* carry the `66` prefix, so the
float32 and float64 FMAs are separated by `W` alone.

`vpackstoreld` and `vpackstorehd` complete the unaligned pair whose load
half (`vloadunpackld`, `vloadunpackhd`) was already here. Same opcodes as
the loads, `D0` and `D4`, and the same `0F38` map, but the stores carry a
`66` prefix and the loads must not have one.

All of these are verified by execution: the AVX-512 co-processor's
translated kernel (`card/examples/avx512_poly.S` in Intel-Phi-AVX512)
uses them and produces results bit-identical to the host's FMA3 hardware
over 65536 lanes. No assembler for this vector ISA exists to check them
against.

## The copy in Intel-Phi-AVX512

Intel-Phi-AVX512 carries this library too (its translator and ggml
backend build on it; the generator, `main.rs`, stays here). The two are
one library: `lib.rs`, `conv.rs` and `transc.rs` are byte for byte the
same in both, and that repository's `make check` compares its copy with
this one whenever it finds this checkout. A change to the encoder is
made in both in the same session. The conversions and transcendentals
were written there (2026-09-22 and 23, for the ggml backend's kernels
and the fused SwiGLU) and brought here on 2026-09-25; nothing here used
them yet, and the generator's output is unchanged by them (its pinned
tests pass).
