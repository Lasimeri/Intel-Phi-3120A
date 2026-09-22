# avx512-demo

A program that uses AVX-512, to be run on a host that has none.

```sh
tcc -o gen tools/gen-avx512-vectors.c -lm && ./gen        # the vectors
gcc -O2 -o avx512-demo tools/avx512-demo.c card/examples/avx512_poly.avx512.s
./avx512-demo                    # dies: Illegal instruction
scripts/phi512.sh ./avx512-demo  # works
```

Nothing in this program knows about the card, the emulator, or the host's
missing instruction set. It is compiled the ordinary way and linked against
an AVX-512 assembly kernel, which the host's assembler encodes perfectly
well even though the host's processor cannot run it.

That is the entire point: this is what an unmodified AVX-512 binary looks
like, and the two runs above are the before and after.

## Measured 2026-09-21, Ryzen 7 5800X

```
$ ./avx512-demo
calling an AVX-512 kernel on a CPU with no AVX-512...
Illegal instruction (core dumped)

$ scripts/phi512.sh --verbose ./avx512-demo
phi512: AVX-512 will be performed in software on this host
calling an AVX-512 kernel on a CPU with no AVX-512...
phi512: performed 135168 AVX-512 instructions
all 65536 lanes bit-identical to AVX-512 hardware
```

The comparison is against `poly_expected.bin`, which was produced by
`fmaf()` on this host's FMA3 hardware, so "bit-identical" means identical
to what real fused-multiply-add hardware computed, not merely
self-consistent.

135168 instructions at about 2101 ns each is 0.284 s, and almost all of
that is the fault itself rather than the arithmetic:
`docs/research/avx512-transparency.md` has the breakdown and where it goes
next.
