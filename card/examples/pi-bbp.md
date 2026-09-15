# pi-bbp.c

Hexadecimal digits of pi by the Bailey-Borwein-Plouffe digit extraction
formula, the card's integer-arithmetic benchmark (the Mandelbrot renderer
in `mandel.c` is the floating point one).

```
pi = sum_{k>=0} 16^-k (4/(8k+1) - 2/(8k+4) - 1/(8k+5) - 1/(8k+6))
```

Multiplying by 16^n and keeping only fractional parts yields the digits
from position n on without computing the ones before it (Bailey, Borwein,
Plouffe, "On the rapid computation of various polylogarithmic constants",
Mathematics of Computation 66, 1997, section 4). The program uses that
to make every 6-digit block an independent work unit:

- Threads take blocks from one mutex-protected counter, largest position
  first: a block's cost grows with its position (n terms of the four
  series, each term a modular exponentiation 16^(n-k) mod (8k+j) with
  about log2(n) squarings), so the expensive blocks must not land in the
  tail of the run.
- `modpow16` is exact 64-bit integer arithmetic: the moduli stay below
  2^32, so every product fits in 64 bits. The default path reduces each
  product with `%` (an integer division); `-DBARRETT` selects Barrett
  reduction with a 64-bit reciprocal computed once per modulus, which
  replaces the divisions by a 128-bit multiply-high and at most two
  subtractions.
- The sums run in `long double`, x87 extended precision with a 64-bit
  mantissa, on both machines (the card's double arithmetic is x87 anyway;
  on the host `long double` also selects x87). Six digits per block leave
  40 bits of margin.
- Certification: the fraction left after a block's six digits and the next
  block's starting fraction are the same number computed independently,
  so their gap measures the rounding error at the scale of the last kept
  digit. The largest gap seen is printed, and every block whose remainder
  lies closer to a digit boundary than sixteen times that gap is counted
  as uncertain (its last digit could be off by one). The count was 0 in
  every run; the worst gap grows from 4.6e-10 at 5k digits to 3.4e-9 at
  160k. A first version summed in `double` with 8-digit blocks: the gaps
  were 1e-4 at 5k digits, which would have left the eighth digit of
  several percent of the blocks in doubt at 200k.
- The first 64 digits are checked against the published constant, and
  the run's exit status is non-zero on any failed check.

Usage, compiled on the card by its own clang:

```
cc -O2 -o pi-bbp pi-bbp.c -lpthread
./pi-bbp DIGITS THREADS [out.txt]          # out.txt gets "3." and the digits
cc -O2 -DBARRETT -o pi-bbp-barrett pi-bbp.c -lpthread
```

Prints wall time, series terms per second, the first 32 digits, the
prefix check, the uncertain block count and the worst neighbour gap.
Cost grows as N^2 log N: 20k digits take 0.49 s on the host's 16 threads,
160k take 40 s. Independent verification used in the results:
`echo "obase=16; scale=6000; 4*a(1)" | bc -l` (17 s, 4983 hex digits).
Results: `docs/results/2026-09-15-pi-bbp.md`.
