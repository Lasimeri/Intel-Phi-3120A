# avx512-conformance

Twelve integer kernels, run by `scripts/phi512-check.sh` to check the
emulator against the same program compiled for this host's own
instruction set.

| kernel | what it exercises |
| --- | --- |
| `and` `or` `xor` | the bitwise lane operations |
| `add` `sub` `mul` | integer arithmetic, including `vpmulld` |
| `shl` `shr` `ushr` | the three shifts, which differ in how they fill |
| `sel` | a compare into a mask, then a merge-masked move: the shape a ternary operator compiles to |
| `cmp` | a compare feeding a count |
| `max` | a compare feeding a select |

Every kernel reduces to a 64-bit sum, so a wrong lane anywhere changes
the printed number. The reduction is deliberate: it drags in the
horizontal fold, which is where `vextracti64x4`, the unpacks and the
scalar EVEX forms live, and those are a different and easily-wrong part
of the instruction set.

`sel` is the one that caught the second real bug. gcc compiles it to
`vpxor` (VEX, does not fault), `vpcmpneqd`, `vpsubd`, and a merge-masked
`vmovdqa32`. The `vpxor` zeroes the whole 512-bit register on real
hardware but only 256 bits here, and the emulator was reading its own
stale upper half.
