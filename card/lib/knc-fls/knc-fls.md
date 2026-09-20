# knc-fls: FastLanes on the card's vector unit

[FastLanes](https://github.com/cwida/FastLanes) is the compression layout
`libknc` was designed against (`docs/research/compression-on-knc.md`).
This directory is the other direction: the real FastLanes library, built
for the card, with its 32-bit decode hot path routed into MVEX kernels.

| File | What it is |
| --- | --- |
| `fls_unffor.cpp` | The replacement dispatcher, appended to FastLanes' generated `fastlanes_gen_unffor.cpp` by `card/userland/components/fastlanes.sh` |
| `fls_check.cpp` | Equivalence against FastLanes' own scalar `unffor`: every width, every input alignment, every base offset |
| `fls_bench.cpp` | Scalar against MVEX, per width, one thread |
| `fls_roundtrip.cpp` | A real file decoded both ways in one process, with a count of how many vectors reached the vector unit |

The kernels themselves are generated: `knc-mvex-gen fastlanes` emits
`knc_fls.S`, which `card/lib/knc/build.sh` assembles into `libknc.a`
alongside the 16-lane codec. `knc.h` declares `knc_fls_unffor` and
`knc_fls_unffor_table`.

## Where the hot path is

`dec_unffor_opr<PT>::Unffor` in `src/expression/decoding_operator.cpp`:

```cpp
uint8_t     bw     = *reinterpret_cast<const bw_t*>(bw_segment_view.data);
const auto* base_p = reinterpret_cast<const PT*>(base_segment_view.data);
const auto* in_p   = reinterpret_cast<const PT*>(bitpacked_segment_view.data);

generated::unffor::fallback::scalar::unffor(in_p, unffored_data, bw, base_p);
```

`unffor` is the only decode primitive the reader calls per vector, and the
ALP operators in `src/expression/alp_expression.cpp` call it too. The
patch replaces the `uint32_t` overload, so both paths move at once and
nothing has to be routed at the call site.

## Only 32-bit values, and that is the hardware

FastLanes instantiates `unffor` for 64, 32, 16 and 8 bit values. Exactly
one of those can go on the vector unit.

| Width | Why |
| --- | --- |
| 8, 16 | Knights Corner has no byte or word integer vector instruction at all. Every integer operation is `D` (32-bit) or `Q` (64-bit) |
| 64 | The whole `Si64` family is `vfixupnanpd`, `vpandnq`, `vpandq`, `vpblendmq`, `vporq`, `vpxorq` (ISA reference 327364-001, appendix D.1.8). No add, no shift, so neither the unpack nor the frame-of-reference step exists |
| 32 | The `Si32` family has `vpaddd`, `vpslld`, `vpsrld`, `vpandd`, `vpord` and the rest (appendix D.1.7) |

This matters more than the count suggests, because FastLanes narrows
physical types: a column whose values fit in a byte is stored as `u16` or
`u8`, not `u32`. On a three-column test round trip on 2026-09-20, two
columns decoded through `pt=4` and one through `pt=2`. Any end-to-end
number has to be read next to the distribution of `sizeof(PT)` over the
vectors it decoded.

## The layout, and why it is the 16-lane kernel run twice

FastLanes' generated `unffor_11bw_32ow_32crw_1uf` reads
`in + (w * 32) + i` and writes `out + (i * 1) + (p * 32)` for `i` in
0..32. Value `i` of a block is therefore in lane `i % 32` at position
`i / 32`, and a lane row is 32 int32, which is 128 bytes, which is two
vectors.

`libknc`'s own codec uses 16 lanes and a 64-byte row. Lanes 0 to 15 and
lanes 16 to 31 of a FastLanes row take the same shift and the same mask
and never interact, so the FastLanes kernel is the 16-lane kernel run
twice at a stride of 128 bytes, once at offset 0 and once at offset 64.
Register pressure and the software-pipelined schedule are unchanged.

## Two differences beyond the addresses

**The base is a scalar.** FastLanes' generated code does
`base_0 = *(a_base_p)` and adds that one value to all 1024;
`libknc`'s FOR mode carries a different base per lane. `vpbroadcastd`
(`MVEX.512.66.0F38.W0 58 /r`) splats it in one instruction, once per
block.

That pointer cannot be passed straight down. A base segment holds four
bytes per vector but starts at an arbitrary byte offset in the file, so
`a_base_p` is routinely not 4-byte aligned; `*(a_base_p)` is a legal
misaligned scalar load and `vpbroadcastd` is `#GP` on such an address
(ISA reference 327364-001, VPBROADCASTD, "Exceptions"). The first
version of the dispatcher did pass it through and took a general
protection fault at the first instruction of `knc_fls_unffor_b11` the
first time a real file was decoded. It now copies the value into a local
with `memcpy` first, which costs one load and one store per 1024 values.
`fls_check` covers base offsets 0 to 3; with the fix reverted it exits
139 there instead of reporting a failure, which is what a `#GP` looks
like from userspace.

**The input is not 64-byte aligned.** `SegmentView::PointTo` sets
`data = data_span.data() + get_offset(...)`, and those offsets accumulate
entry-point arrays that can be one byte per vector, so a bitpacked
segment starts at an arbitrary multiple of four inside the file buffer.
Measured on 2026-09-20 with a probe in `Unffor`, the residues modulo 64
were 8, 48 and 60 for the three columns, constant per segment because
each vector advances by `128 * bw` bytes. `vmovaps` faults on all three,
so every load in these kernels is `vloadunpackld` plus `vloadunpackhd`.

The output does not need it: `unffored_data` is
`alignas(64) PT unffored_data[CFG::VEC_SZ]` in
`src/include/fls/expression/decoding_operator.hpp`, and the ALP buffers
(`unffor_arr`, `unffor_right_arr`, `unffor_left_arr`) are all `alignas(64)`
too. The dispatcher checks it anyway and falls back to scalar if it ever
stops being true, because that turns a future upstream change into a slow
path rather than a fault.

## Inputs below 4-byte alignment: staged, not given up on

`vloadunpackld` needs the address to be element-aligned, which is 4 bytes
here, and about half the columns of a real file are not. Measured on the
example dataset, 59 of 118 `unffor` calls. The dispatcher copies the
packed bytes into an aligned scratch buffer and runs the kernel on that,
rather than falling back to scalar.

The copy is cheap because it is of the *compressed* data: at most 4096
bytes at `bw = 32`, 1408 at `bw = 11`, against 4096 bytes of output. It
is worth about half the direct win: 3.24x against 6.61x at `bw = 11`,
where giving up would be 1.00x. End to end it was worth 1.35x to 1.80x.

The exception is `bw = 32`, where the kernel is already a copy and
staging makes it two, so the staged path runs at 0.98x. Not
special-cased: the branch would cost more than the one width is worth.

The scratch is `KNC_FLS_BLOCK + 16` values rather than `KNC_FLS_BLOCK`,
so the kernel's 60-byte tail read stays inside the array. It is a stack
buffer, not a static one, because FastLanes' decode is per-expression and
a shared one would be a data race.

`valignd` would avoid the copy in principle, by loading aligned lines and
realigning in registers. It cannot be used: the misalignment is a
run-time value and `valignd` takes an immediate, so it would mean sixteen
variants of every kernel.

## Cost of the unaligned loads

One extra instruction per 64-byte load: `2 * bw` loads per block become
`4 * bw`. At `bw = 11` that is 22 instructions added to roughly 220.

That is cheaper than staging every input up to 64-byte alignment, which
would cost unaligned loads plus aligned stores plus the aligned loads
afterwards, so the load pair is the right default and the copy above is
reserved for the inputs the pair cannot take at all.

The pair reads up to 60 bytes past the end of the input, because it always
touches the whole of both 64-byte lines around an address (ISA reference
327364-001, VLOADUNPACKLD: "the memory region accessed will always be
between `linear_address & (~0x3F)` and `(linear_address & (~0x3F)) + 63`").
That needs nothing from the caller. Those bytes share a 64-byte line with
the last byte of the input, a line never crosses a page, so the read
cannot reach an unmapped one; the remaining case, an address that is
itself 64-byte aligned, the same section exempts from `#PF` outright.

## Checking it

`fls_check` runs FastLanes' scalar `unffor` and the MVEX one over the
same input and compares 4096 bytes, in four sweeps: every width 0 to 32
at every 4-byte offset 0 to 60; every byte offset 0 to 63 at five widths,
which is the staging path; the four base-pointer offsets; and an output
pointer that is not 64-byte aligned. The reference is FastLanes' own
generated code under the name the patch renamed it to, not a model
written here, so a misreading of the layout cannot hide in it. It also
calls the patched dispatcher, so the guard and the fallback are on the
same evidence.

Run it with `card/userland/components/fastlanes.sh`, which builds both
programs into the package tree.
