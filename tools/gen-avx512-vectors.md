# gen-avx512-vectors

Writes the test vectors and the expected results for the translated AVX-512
kernels in `card/examples`.

```
tcc -o gen-avx512-vectors tools/gen-avx512-vectors.c -lm
./gen-avx512-vectors
```

Seven files land in the working directory: `a.bin`, `b.bin`, `c.bin` and
`expected.bin` for the fused multiply-add kernel, and `x.bin`, `coef.bin`
and `poly_expected.bin` for the polynomial one.

## Why the expected values are computed here and not on the card

The claim being tested is that the card reproduces AVX-512 results **bit for
bit**, so the reference has to come from somewhere that is not the card.
This host is a Ryzen 7 5800X: it has no AVX-512, but it does have FMA3, and
glibc lowers `fmaf()` onto it. So `expected.bin` holds results produced by
real fused-multiply-add hardware, single-rounded, which is exactly what an
AVX-512 `vfmadd` would have produced for the same inputs.

Computing the reference on the card instead would prove nothing: it would
compare the card against itself.

## The four regions of the multiply-add vectors

A failure should say which class of value broke, so the 4096 inputs are
divided:

| index | region | what it exercises |
| --- | --- | --- |
| 0 to 1023 | ordinary | exponents near 1.0, the easy case |
| 1024 to 2047 | wide range | operands far apart in magnitude, so the add has something to round |
| 2048 to 3071 | denormal | inputs below the smallest normal, which `MXCSR.DAZ` and `MXCSR.FZ` govern on both machines |
| 3072 to 4095 | specials | both infinities, a signalling NaN, both zeros, the smallest denormal, the largest finite |

The specials region is the one that would catch a translator that quietly
substituted an instruction with a different NaN or signed-zero rule, which
is the mistake `vmaxps` invites (`docs/research/avx512-on-knc.md`).
