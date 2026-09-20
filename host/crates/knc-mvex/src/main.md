# main.rs (knc-mvex-gen)

Generates every file that contains Knights Corner vector code, so that the
encoder in lib.rs is the single source of truth:

| command | output | contents |
| --- | --- | --- |
| `knc-mvex-gen probe` | `card/examples/vpu_probe.S` | `vpu_probe` (one instruction of each kind, checked by vpu_probe.c), `vpu_set` and `vpu_get` (load or store all 32 zmm and 8 mask registers, used by vpu_state.c) |
| `knc-mvex-gen int-probe` | `card/examples/vpu_int.S` | `vpu_int_probe`, one int32 instruction per output slot, checked lane by lane by vpu_int.c |
| `knc-mvex-gen mandel` | `card/examples/mandel_vpu.S` | `mandel8_vpu`, the Mandelbrot inner loop on 8 doubles |
| `knc-mvex-gen memcpy` | `card/examples/vpu_memcpy.S` | `knc_memcpy64`, 64-byte aligned block copy |
| `knc-mvex-gen bitunpack` | `card/examples/bitunpack.S` | `knc_unpack_b5`, `knc_unpack_b11`, `knc_unpack_b16`: 1024 values of N bits into 1024 int32 |
| `knc-mvex-gen kernel-header` | `arch/x86/include/asm/knc_vpu.h` in the kernel tree (patch 0024) | `knc_vpu_save` / `knc_vpu_restore` inline assembly and the state layout constants |

## Scheduling is part of the generator

`unpack_kernel` emits phase by phase rather than value by value: all the
first shifts, then all the second shifts, then all the ors, then all the
masks, then all the stores, across a group of eight values. The core is
in order, so a dependent instruction stalls until its input is ready, and
the value-at-a-time version of the same kernel ran at 1.07 values per
cycle against 1.79 for the pipelined one, a 1.8x difference from
reordering alone (`docs/results/2026-09-20-bitunpack.md`). Any kernel
added here wants the same treatment; there is no out-of-order window and
no compiler between this code and the card.

Scalar instructions (moves, loop control, `lea`) are ordinary AT&T
mnemonics for the card's clang; only the vector and mask instructions are
`.byte` lines, each with its Intel-syntax mnemonic as a comment. The
generated files are committed so that the card can be used without
running the generator.

`cargo test -p knc-mvex` compares the generator's output with every
committed `.S` file and with the header inside patch 0024, so after any
encoder change the tests fail until all of them are regenerated: the `.S`
files in place, the header into the patch (and the kernel tree, then a
kernel rebuild). The probe files are then run on the card before the
change is accepted (`docs/results/2026-09-15-vpu.md` and
`docs/results/2026-09-20-mvex-integer.md` are the procedure).
