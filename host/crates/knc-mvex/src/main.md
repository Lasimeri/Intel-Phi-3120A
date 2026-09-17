# main.rs (knc-mvex-gen)

Generates the three files that contain Knights Corner vector code, so
that the encoder in lib.rs is the single source of truth:

| command | output | contents |
| --- | --- | --- |
| `knc-mvex-gen probe` | `card/examples/vpu_probe.S` | `vpu_probe` (one instruction of each kind, checked by vpu_probe.c), `vpu_set` and `vpu_get` (load or store all 32 zmm and 8 mask registers, used by vpu_state.c) |
| `knc-mvex-gen mandel` | `card/examples/mandel_vpu.S` | `mandel8_vpu`, the Mandelbrot inner loop on 8 doubles |
| `knc-mvex-gen kernel-header` | `arch/x86/include/asm/knc_vpu.h` in the kernel tree (patch 0024) | `knc_vpu_save` / `knc_vpu_restore` inline assembly and the state layout constants |

Scalar instructions (moves, loop control, `lea`) are ordinary AT&T
mnemonics for the card's clang; only the vector and mask instructions are
`.byte` lines, each with its Intel-syntax mnemonic as a comment. The
generated files are committed so that the card can be used without
running the generator.

`cargo test -p knc-mvex` compares the generator's output with the two
committed `.S` files and with the header inside patch 0024, so after any
encoder change the tests fail until all three are regenerated: the two
`.S` files in place, the header into the patch (and the kernel tree, then
a kernel rebuild). The mandel and probe files are then run on the card
before the change is accepted (`docs/results/2026-09-15-vpu.md` is the
procedure).
