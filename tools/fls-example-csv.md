# fls-example-csv.c

Writes the CSV that `card/userland/components/fastlanes.sh` packages as
`/opt/phi/share/fls-example/data.csv`, the dataset `fls_roundtrip` decodes
on the card.

```sh
tcc -run tools/fls-example-csv.c 60000 > data.csv
```

Three integer columns, deterministic from a fixed seed, pipe separated,
one header-free row per line. The schema that goes with it is written by
the component script.

## Why the values are large

FastLanes chooses a physical type by magnitude, not by packed width. A
column whose values fit in 16 bits is stored as `u16`, and Knights Corner
has no 16-bit integer vector instruction at all: every integer vector
operation is 32 or 64 bit (ISA reference 327364-001, appendix D.1), and
the 64-bit set has no add and no shift. Only `u32` reaches the MVEX
kernels.

So every column here sits above 65535, while the residue after frame of
reference is small:

| Column | Range | Physical type | Packed width |
| --- | --- | --- | --- |
| a | 100000 to 102047 | `u32` | 11 |
| b | 7000000 to 7131071 | `u32` | 17 |
| c | 200000 to 200031 | `u32` | 5 |

This is deliberately the good case for the vector path. A column of small
integers would be narrowed to `u16` and would decode entirely on
FastLanes' scalar code, at scalar speed. The results document
(`docs/results/2026-09-20-fastlanes-vpu.md`) states that next to the
number.

Verified on the host on 2026-09-20 with a probe inside
`dec_unffor_opr<PT>::Unffor`: columns of this shape report `pt=4`, and a
column of 8-bit values in the same file reports `pt=2`.
