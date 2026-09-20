# delta-stride.c

What the interleaved layout costs DELTA, in bits per value.

```sh
tcc -run tools/delta-stride.c
```

`libknc` puts value `i` in lane `i % 16`, so "the value before it in the
same lane" is the value **sixteen positions earlier** in the caller's array,
not its neighbour. DELTA therefore stores stride-16 differences where a
transposed layout (lane `L` holding values `L*64` to `L*64+63`) would store
stride-1 ones. The packed width follows the widest difference in the block,
so a sorted column pays for that.

This runs on the host because it is arithmetic about the data, not about
the card: no vector instruction is involved.

## Result, 2026-09-20

| Column | max d1 | bits | max d16 | bits | cost |
| --- | --- | --- | --- | --- | --- |
| dense ids, gaps 1 to 2 | 2 | 2 | 26 | 5 | 3 |
| timestamps, 1 to 20 ms | 20 | 5 | 230 | 8 | 3 |
| sorted keys, gaps to 1000 | 1000 | 10 | 11314 | 14 | 4 |
| strictly sequential | 1 | 1 | 16 | 5 | 4 |

**Three to four extra bits per value**, which is `log2(16)` as expected:
sixteen steps of a random walk have roughly sixteen times the span of one.
On a strictly sequential column it is the difference between 1 bit and 5,
which is a 5x worse ratio on the best case DELTA has.

## Why the layout is still the interleaved one

The transposed alternative keeps every lane at the same bit offset, so
bit-packing is unchanged and the cost above disappears. What it breaks is
the store: sixteen consecutive integers become a stride-64 scatter, and
`vscatterd` serialises on this card
(`docs/results/2026-09-20-zstd-vpu.md`). That is the same reason FastLanes'
Unified Transposed Layout was not adopted, and it costs more than four bits
per value.

The tradeoff is real and it is a tradeoff, not an oversight: DELTA here is
worth less than DELTA on a machine that can scatter cheaply.
