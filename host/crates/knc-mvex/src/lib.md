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

Instructions covered: `vmovaps` load/store (the form Intel's kernel
uses), `vmovapd` load/store/move, `vaddpd`, `vsubpd`, `vmulpd`,
`vfmadd213pd`, `vfmadd231pd`, `vcmppd` (with its write mask acting as an
AND on the result: a clear mask bit clears the result bit, table 6.3 and
the note under it), `kmov` in all three directions, `kortest`.

Each function returns an `Insn` with the bytes and the Intel-syntax text
(`[rbp-64]` for a negative displacement); `gas()` renders a `.byte` line
with the text as a comment, `c_string()` a C string literal for inline
assembly.

## Tests

Three sources pin the bytes, in decreasing strength:

1. Hardware: `probe_bytes_verified_on_the_card` holds the bytes of
   `card/examples/vpu_probe.S`, which ran on the card on 2026-09-15 with
   every lane checked (`docs/results/2026-09-15-vpu.md`).
2. Intel's macros: the `vmovaps` load and store for all 32 registers and
   the `kmov` r32 forms for all 8 masks reproduce `mic_ni.h` byte for byte.
3. The document: the extension bits for registers 8 to 31, memory bases
   above `rdi`, `kmov k, k` and `vfmadd231pd` (the one form no generated
   file uses) follow section 3.3 and the opcode column of chapter 6, and
   have not run on the card.

The generator's tests (`main.md`) compare its output with the committed
files, so an encoder change is visible until they are regenerated.
