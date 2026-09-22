# avx512-xlate: the command

```
avx512-xlate <input.s> [--name FN] [--out OUT.S]
```

Assembles an AVX-512 function, rewrites every EVEX instruction into Knights
Corner MVEX, and writes assembly the card's compiler will accept.

```
avx512-xlate kernel.avx512.s --name my_kernel --out my_kernel.S
cc -O2 -o prog harness.c my_kernel.S      # on the card
```

## Why so little has to be translated

Knights Corner is x86-64. The scalar half of a vectorised function, the
pointer arithmetic, the loop counter, the compare and the branch, is
already valid card code. Only the EVEX instructions have to change. On the
polynomial kernel in `card/examples` that is 264 instructions rewritten and
7 passed through untouched.

The host assembler is what makes the front of the pipeline work: `llvm-mc`
encodes AVX-512 perfectly well on a machine that cannot execute a single
AVX-512 instruction. That gap between what the host can *encode* and what it
can *run* is the whole reason this tool exists.

## Branches, and why bytes cannot simply be copied

An unaligned AVX-512 load becomes two card instructions, so instruction
lengths change during the rewrite and every relative branch displacement in
the original would be wrong. So the tool decodes twice: once to collect the
address of every branch target, and once to emit, planting a label at each
target and emitting branches as text against those labels. The card's
assembler resolves them.

## The passthrough is checked, not trusted

A scalar instruction is only safe to copy if Knights Corner still has it,
and Knights Corner deleted parts of the x86-64 baseline. `knc_illegal`
rejects `cmov` (which is in the 64-bit baseline and which compilers emit
freely), `pause`, the three fences, scalar prefetch, `popcnt`, `lzcnt`,
`tzcnt`, `clflush`, and anything VEX-encoded, since the card has no `xmm` or
`ymm` registers at all. `docs/research/isa-deletions.md` carries the full
list with its sources.

## Limits worth knowing before using it

- One function per input file; the emitted symbol is whatever `--name` says.
- Memory operands must be `[base + displacement]`. The MVEX encoder emits no
  SIB byte, so indexed addressing has to be strength-reduced to a pointer
  increment first. `rsp` and `r12` cannot be the base for the same reason.
- The instruction table is a subset of AVX-512F, not all of it. Everything
  outside it is refused by name.
