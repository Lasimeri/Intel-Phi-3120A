# 2026-09-20: FastLanes on Knights Corner, as far as this card can take it

The last of the three steps: bit packing at every width was the bottom
layer, and this adds the cascaded encodings that make it a compression
scheme. It also says plainly what was *not* implemented and why.

## What FastLanes is, and which parts are here

`docs/research/compression-on-knc.md` picked FastLanes because its operator
set is exactly what this card has: load, store, shift, and, or, xor, add, at
32-bit lanes. Four things make up the published design.

| Part | Here |
| --- | --- |
| Lane-interleaved layout, all lanes at one bit offset | **Yes**, at 512 bits with 16 lanes |
| Interleaved bit (un)packing, every width | **Yes**, widths 1 to 32, both directions |
| FOR and DELTA cascading | **Yes** |
| RLE and DICT | **No**, and not worth it here |
| The Unified Transposed Layout | **No**, deliberately |

## The layout, stated precisely

The layout is FastLanes' central idea: value `i` lives in lane `i % 16` at
position `i / 16`, so all sixteen lanes sit at the same bit offset at the
same time, one shift-and-mask pair serves all of them, and nothing crosses a
lane. It is that idea instantiated at 512 bits, with 16 lanes and the
identity tuple order.

It is **not** FastLanes' Unified Transposed Layout, so packed bytes are not
interchangeable with the reference implementation. The UTL reorders 1024
tuples into eight 8x16 transposed blocks in the order 04261537, and exists
so that one tuple order serves lane widths 8, 16, 32 and 64 across all the
columns of a table. This machine has no 8-bit or 16-bit integer vector
instructions at all, so two of the four widths it unifies cannot be
vectorised here under any layout, and the unification buys this card
nothing. Adopting it would also turn every store from sixteen consecutive
integers into a stride-64 scatter, and `vscatterd` serialises on this card.

## FOR and DELTA

Both decode fold into the unpack kernel, so each costs one instruction per
sixteen values:

- **FOR**, `out[i] = unpacked[i] + base[i % 16]`: sixteen independent
  `vpaddd`, one per value in the group.
- **DELTA**, the running sum along positions within each lane: a `vpaddd`
  chain 64 deep. It cannot be software-pipelined, because the chain is the
  algorithm. It survives because all sixteen lanes run their own chain at
  once and the bit-unpacking of the next group does not depend on it, so
  the chain hides inside work that has to happen anyway.

Encoding is a separate pass rather than being folded into the packer. A
value that straddles two packed words is read twice, so folding the
subtraction in would apply it twice; a separate pass costs one instruction
per sixteen values and cannot get that wrong. Decode is the direction that
has to be fast, and there it *is* folded in.

Both require the stored residue to fit in the bit width, unsigned. Delta
therefore wants ascending data; a column that goes down as well as up needs
a zigzag transform, which this library does not provide.

## What the layout costs DELTA, measured

Choosing the interleaved order over a transposed one has a third
consequence beyond the two above, and it is a real cost rather than a free
simplification.

Value `i` lives in lane `i % 16`, so the value before it *in its lane* is
sixteen positions earlier in the caller's array, not its neighbour. DELTA
therefore stores stride-16 differences. For the sorted columns DELTA exists
to compress, sixteen steps of a walk span roughly sixteen times one step,
so the bit width goes up by about `log2(16)`.

`tools/delta-stride.c` measures it:

| Column | stride 1 | stride 16 | cost |
| --- | --- | --- | --- |
| dense ids, gaps 1 to 2 | 2 bits | 5 bits | 3 |
| timestamps, 1 to 20 ms | 5 bits | 8 bits | 3 |
| sorted keys, gaps to 1000 | 10 bits | 14 bits | 4 |
| strictly sequential | 1 bit | 5 bits | 4 |

Three to four extra bits per value. On a strictly sequential column, which
is the best case DELTA has anywhere, it is 1 bit against 5.

A transposed layout (lane `L` holding values `L*64` to `L*64+63`) removes
this entirely and keeps every lane at the same bit offset, so bit-packing
would be unchanged. What it breaks is the store: sixteen consecutive
integers become a stride-64 scatter, and `vscatterd` serialises on this
card. That costs more than four bits per value, so the interleaved order
stands, but **DELTA here is worth less than DELTA on a machine that can
scatter cheaply**, and that is a property of this card rather than of the
encoding.

## Correctness

`knc_test.c` on the card: 32 widths, two directions, plain, FOR and DELTA.
**0 checks failed**, first run.

Every direction is crossed with an independent scalar model of the layout
and of the transform, written from the definitions, because an encoder and
its matching decoder will happily agree on a wrong idea. For the plain
codec the packed bytes are also compared with the model's byte for byte,
which is the check a round trip cannot make.

## Measured

Millions of values per second, 11 bits, on the card.

| | unpack | FOR | DELTA | pack | scalar |
| --- | --- | --- | --- | --- | --- |
| 1 thread, in L1 | 1829 | 1459 | 1091 | 1402 | 41 |
| 228 threads, streaming | 10463 | 10456 | 10643 | 14634 | 3495 |

And across widths at 228 threads:

| bits | unpack | FOR | DELTA | pack |
| --- | --- | --- | --- | --- |
| 1 | 12691 | 12856 | 12407 | 11439 |
| 8 | 11112 | 11212 | 11260 | 17039 |
| 11 | 10463 | 10456 | 10643 | 14634 |
| 16 | 9863 | 9998 | 9965 | 12307 |
| 32 | 8267 | 8151 | 8103 | 8120 |

## The finding

**The transforms cost 20 and 40 percent in cache and nothing at all at 228
threads.** In L1, FOR takes unpack from 1829 to 1459 (0.80x) and DELTA to
1091 (0.60x), which is the dependent chain showing exactly where it should.
At 228 threads all three are the same number to within noise.

That is not a disappointment, it is the cleanest evidence in this project
that the card's vector work is memory-bound at full occupancy. Work that
costs 40 percent when the data is in cache and zero when it is not was
waiting on memory either way. It also means **cascading is free here**: a
column that compresses better because it is delta-coded costs nothing to
decode, and moving less data is the only lever left.

**The card reaches 0.28 of the host.** 10463 M values per second on 228
threads against 37200 on the host's 16, where the host runs the same
lane-interleaved source through its own auto-vectoriser, which is what
FastLanes is designed to do and what this card cannot do at all. Still the
best ratio this project has measured, narrowly ahead of xz's 0.22 to 0.26,
and still a loss.

## Why not RLE and DICT

Dictionary decode is a gather. `vgatherd` exists on this card and
serialises internally, one element at a time, which is the single operation
that would undo everything the layout is for. RLE decoding in FastLanes is
built on the same gather. Both are better left scalar here, and a column
that wants them is a column this card should not be given.

## What a real codec still needs

A container: the bit width per block, the frame base, the delta seed, and
which cascade was applied all have to be stored somewhere, and that is
format design rather than kernel work. `libknc` deliberately stops at the
kernels, because the format is the part that should not be invented twice.

Zigzag, so delta works on data that descends.

A host-side implementation of the same layout for comparison at more than
one width, and an end-to-end number on a real column rather than a
synthetic one. Everything above is a micro-benchmark.
