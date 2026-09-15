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
| P0 | `R X B R' 0 0 m m` | register extensions, stored inverted; `mm` = 01 for 0F, 10 for 0F38, 11 for 0F3A |
| P1 | `W v v v v 0 p p` | `vvvv` = first source register, inverted; bit 2 is 0 (EVEX has 1 here); `pp` = 00 none, 01 66H |
| P2 | `E S S S V' a a a` | eviction hint, swizzle/conversion, `vvvv` bit 4 inverted, write mask `k` |

Then the opcode, ModRM, an optional disp32 and an optional imm8. A vector
register in ModRM.reg is extended by R and R', one in ModRM.r/m by X and
B. Only the plain forms are produced: `E = 0`, `SSS = 000` (no swizzle,
no conversion, round to nearest), memory operands as `[base + disp32]`
with mod = 10 so the disp8*N compression never applies. The base register
may not be `rsp` or `r12` (SIB byte). Mask register moves and `kortest`
are two-byte VEX instructions (`C5 F8 opcode ModRM`), limited to the
first eight general registers.

Instructions covered: `vmovaps` load/store (the form Intel's kernel
uses), `vmovapd` load/store/move, `vaddpd`, `vsubpd`, `vmulpd`,
`vfmadd213pd`, `vfmadd231pd`, `vcmppd` (with its write mask acting as an
AND on the result, table 6.3), `kmov` in all three directions, `kortest`.

Each function returns an `Insn` with the bytes and the Intel-syntax text;
`gas()` renders a `.byte` line with the text as a comment, `c_string()` a
C string literal for inline assembly.

Verified on the hardware by `card/examples/vpu_probe.c` (2026-09-15: all
seven checks pass on the card).
