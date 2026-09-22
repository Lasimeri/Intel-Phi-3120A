# avx512-seamless-test.c

The user's-eye test of the transparent path: an ordinary program written
with AVX-512 intrinsics, compiled with `-mavx512f`, that checks its own
answers against scalar references compiled with AVX-512 forbidden
(`target("no-avx512f")`, so the references run natively and cannot be
intercepted). It knows nothing about the card, the emulator or the
wrapper. Run bare on the 5800X it dies with SIGILL; under
`scripts/phi512.sh` it runs to completion and says whether every lane is
right and how long the AVX-512 section took.

Three kernels: a degree-30 Horner polynomial (the shape the card's
compiled-in kernel has), a dot product with a tree reduce, and an
integer update through a mask register. The dot reference reproduces
the reduce's rounding order, so "bit-identical" is the standard for all
three.

```
gcc -O2 -mavx512f -mno-avx512vl -mno-avx512bw -mno-avx512dq -mno-avx512cd \
    -o /tmp/avx512-seamless-test tools/avx512-seamless-test.c -lm
scripts/phi512.sh --verbose /tmp/avx512-seamless-test 65536
```

What it establishes, and what it cannot: it proves the program's AVX-512
instructions were performed correctly without the host having them. It
cannot tell WHERE they were performed; that is read from the other side
(`phi -c N vpu log`, `phi -c N traffic`) before and after the run. See
`docs/results/2026-09-22-seamless-test.md` for the answer on this host.
