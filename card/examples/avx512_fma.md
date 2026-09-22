# avx512_fma: does the card reproduce AVX-512 bits exactly?

`avx512_fma.avx512.s` is an AVX-512 function. This host cannot execute a
single instruction of it. `avx512_fma.S` is what `avx512-xlate` turned it
into, and `avx512_fma.c` checks the card's answer against what the host's
FMA3 hardware produced.

```
tcc -o gen tools/gen-avx512-vectors.c -lm && ./gen     # on the host
host/target/debug/avx512-xlate card/examples/avx512_fma.avx512.s \
    --name fma_kernel --out card/examples/avx512_fma.S
# copy the .bin files, the .c and the .S to the card, then
cc -O2 -o avx512_fma avx512_fma.c avx512_fma.S && ./avx512_fma
```

The kernel is `d[i] = a[i]*b[i] + c[i]` in float32, written with `vmovups`
and `vfmadd231ps`. Five EVEX instructions translate, nine scalar
instructions pass through.

## What it actually tests

The `vmovups` choice is deliberate. Knights Corner has no unaligned 512-bit
move, so each one becomes `vloadunpackld` plus `vloadunpackhd`, the pair
that names the two cache lines an access can straddle. The harness runs the
kernel at byte offsets 0, 4, 32 and 60 inside its buffers, so the pair is
exercised both when it straddles a line and when it does not.

The inputs include denormals, both infinities, a signalling NaN, both
zeros, and the largest finite float, grouped into regions so a failure
reports which class broke.

Buffers carry 64 bytes of slack past the end, because the load half of the
pair always reads the whole of both lines, which for the last iteration is
one line beyond the data.

## Measured 2026-09-21

```
offset  0: all 4096 lanes bit-identical
offset  4: all 4096 lanes bit-identical
offset 32: all 4096 lanes bit-identical
offset 60: all 4096 lanes bit-identical
bit-exact at every alignment
```

## What the unaligned pair costs

Same kernel translated twice, once from `vmovups` and once from `vmovaps`,
both given 64-byte aligned data, single thread:

| working set | unaligned pair | aligned | cost |
| --- | --- | --- | --- |
| L2 resident | 2623.72 MB/s | 2867.49 MB/s | 1.09x |
| beyond cache | 1549.06 MB/s | 1659.11 MB/s | 1.07x |

Two instructions instead of one costs 7 to 9 percent, not 100 percent. On
aligned data `vloadunpackhd` addresses the line 64 bytes up, which the next
iteration reads anyway, so the second instruction behaves like a prefetch
rather than like waste. The practical consequence is that the translator
does not need to prove alignment to emit code that performs well.
