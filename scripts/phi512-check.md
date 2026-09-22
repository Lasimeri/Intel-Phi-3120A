# phi512-check.sh

Does the emulator agree with the hardware?

```sh
scripts/phi512-check.sh            # the built-in conformance programs
scripts/phi512-check.sh my.c       # any C file that prints its results
```

Compiles the same C twice: once with `-march=native`, so it runs on this
host directly, and once with `-mavx512f`, so it does not. Runs the first
normally and the second under `phi512.sh`, and compares the output.

The point of using one source file is that the reference is not a model of
what AVX-512 should do, written by the same person who wrote the emulator.
It is the same program, the same compiler and the same optimisation level,
producing the same arithmetic through a different instruction set. Any
difference belongs to the emulator.

## Why the AVX-512 build is held to F

`-mno-avx512vl -mno-avx512bw -mno-avx512dq -mno-avx512cd` keep gcc inside
AVX-512F, which is the subset the emulator implements. Without them the
compiler reaches for byte and word instructions that are genuinely not
covered, and the failure is a gap rather than a disagreement.

## What the two programs cover

`tools/avx512-conformance.c` runs twelve integer kernels: bitwise and,
or, xor, add, subtract, multiply, three shifts, a masked select, a
compare, and a maximum. Each reduces to a 64-bit sum, so a wrong lane
anywhere changes the printed number.

`tools/avx512-conformance-float.c` runs three float kernels: a
conditional sum that mixes multiply and divide, an integer kernel with a
mask and a shift, and a widening one that converts float32 to float64.

Between them they exercised, and caught, both of the bugs that mattered:
the low 256 bits of every register being real hardware, and a VEX write
zeroing the upper half of a register the emulator owns.
