# avx512-xlate: the translation table

Turns one decoded AVX-512 instruction into the Knights Corner instructions
that reproduce it. `docs/research/avx512-on-knc.md` is the evidence behind
every case here; this file is where those findings become code.

## The shape of the problem

MVEX is the ancestor of EVEX. Both are a `62H` prefix followed by three
payload bytes, both address 32 512-bit registers and 8 mask registers, and
for the arithmetic that exists on both machines the rewrite touches only
bit 2 of P1, which EVEX fixes at 1 and MVEX does not use, and the P2 byte:

| | bit 7 | bits 6-4 | bit 3 | bits 2-0 |
| --- | --- | --- | --- | --- |
| EVEX P2 | `z` | `L'L` (6-5), `b` (4) | `V'` | `aaa` |
| MVEX P2 | `E` | `SSS` | `V'` | `aaa` |

| | bit 7 | bits 6-3 | bit 2 | bits 1-0 |
| --- | --- | --- | --- | --- |
| EVEX P1 | `W` | `vvvv` | fixed `1` | `pp` |
| MVEX P1 | `W` | `vvvv` | unused | `pp` |

So `vaddps zmm0, zmm1, zmm2` is `62 f1 74 48 58 c2` as AVX-512 and
`62 f1 70 08 58 c2` on the card. Two bits. That is the easy half and it
covers most of AVX-512F.

## The cases that are not a re-encode

**Unaligned access.** The card has no `vmovups`. Each one becomes a pair,
`vloadunpackld` plus `vloadunpackhd` to load and `vpackstoreld` plus
`vpackstorehd` to store, with the high half addressing 64 bytes above the
low. The pair exists because of cache geometry: each half takes the part of
the 64-byte stream that falls in one 64-byte line, so naming both lines
takes two instructions. Measured cost on aligned data, 1.07x to 1.09x, far
less than the doubled instruction count suggests, because the high half
touches the line the next iteration wants anyway.

Note the encoding trap: the loads carry no legacy prefix and the stores
carry `66`. Same opcodes, same map, different prefix.

**Zeroing masking.** AVX-512 `{z}` writes zeros into masked-off lanes. The
card has merging masking only. This is refused rather than approximated,
because covering it means clearing the destination first, which is not a
local rewrite and changes register pressure.

**Embedded rounding and broadcast.** Both are expressible on the card, as
MVEX static rounding (`EH=1` plus `SSS`) and as an MVEX swizzle
respectively. Neither is wired through the encoder yet, so both are refused
by name. Guessing here would silently change results, which is the one
failure this translator must never have.

**Divide and square root.** `vdivps`, `vdivpd`, `vsqrtps`, `vsqrtpd` have no
card equivalent of any kind. Refused with a pointer to the Newton-Raphson
expansion they need.

**Minimum and maximum.** The card's `vgmaxps` returns the operand that is
not NaN; AVX-512's `vmaxps` returns the second source. Refused until the
compare-and-blend expansion exists, because substituting one for the other
would be wrong only on NaN inputs, which is the kind of bug that survives
testing.

**Gather and scatter.** The card completes only a subset of elements per
issue and must be looped until the mask clears. That adds control flow the
caller's region model has to carry, so it is refused here rather than
silently emitted as a single instruction that does part of the job.

## The rule the module is built on

Anything that cannot be translated is refused by name with a reason.
Nothing is approximated. A wrong answer that looks like a right answer is
the only outcome that would make the whole exercise worthless, so the
failure mode is always a refusal a caller can read.
