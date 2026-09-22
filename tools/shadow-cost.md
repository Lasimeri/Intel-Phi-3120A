# shadow-cost

What it costs to execute AVX-512 on a host that has none.

```sh
gcc -O3 -march=native -o shadow-cost tools/shadow-cost.c && ./shadow-cost
```

The host has no `zmm` registers, so any layer running AVX-512 on its behalf
keeps the program's 32 vector registers in memory (a shadow register file)
and does the arithmetic as pairs of 256-bit AVX2 operations. This measures
the two ways of doing that against what the host would have managed natively.

Four variants of the same degree-30 Horner evaluation:

| variant | what it models |
| --- | --- |
| `native AVX2 8-lane` | an ordinary AVX2 loop |
| `native AVX2 16-lane` | the same with two independent chains, which is the baseline, because splitting 512 bits into two 256-bit halves produces that parallelism for free and comparing against the 8-lane version would credit the translator for it |
| `region translated` | a whole region translated at once: live-in read from the shadow file, intermediates kept in `ymm`, live-out written back |
| `per instruction` | each AVX-512 instruction translated alone, both sources read from the shadow file and the result written back, with a barrier so nothing carries between what are separately patched sites |

Measured 2026-09-21 on a Ryzen 7 5800X: region translation costs 1.11x,
per-instruction costs 6.54x. `docs/research/avx512-transparency.md` has the
table and what follows from it.

The barrier in the per-instruction variant is the point of it. Without it
the compiler keeps values in registers across the whole loop, which is
exactly what a per-instruction layer cannot do, and the number would be
meaninglessly good.
