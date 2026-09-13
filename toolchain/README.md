# toolchain/

How code for the card is compiled. Everything here follows
`docs/decisions/0002-64bit-userland-x87-abi.md`.

| Directory | Contents |
| --- | --- |
| `llvm/` | The LLVM/clang patch design (x87 float return without SSE, CMOV not implied by 64-bit mode) and the build recipe. Patches land in `llvm/patches/` in phase P2. |
| `rust/` | The custom Rust target `x86_64-knc-linux-musl` and how to build `std` for it. |
| `clang/` | `knc-cc`, the driver wrapper that bakes in the flags. |

## Build order (phase P2)

1. Patched LLVM: `llvm/README.md`. Produces `toolchain/build/llvm/`.
2. musl with `knc-cc`: `card/userland/components/musl.md`. Produces
   `toolchain/build/sysroot/`.
3. Rust `std` for the custom target with `-Zbuild-std` (needs a nightly
   `rustup` toolchain; `scripts/setup-arch.sh` with `PHI_RUSTUP=1`).
4. Exit criterion: static `hello` in C and Rust, both `phi-isa-audit` clean.

## What the audit will still flag in correct output

Nothing, by construction: `knc-cc` disables every feature in the deletion
list and `-fcf-protection=none` removes `endbr64`. Alignment padding uses
multi-byte NOPs unless the assembler is told otherwise; if the on-card test
in P2 shows `0F 1F` NOPs fault, add `-Wa,-mno-...` equivalent (LLVM MC
option `--x86-pad-for-align=false` and `-mllvm -x86-use-single-byte-nop` are
the candidates) and re-audit.
