# knc-cc

Driver wrapper so that every C/C++ build for the card gets the same flags.
`CC=knc-cc` is all a project's build system needs.

| Flag | Reason |
| --- | --- |
| `--target=x86_64-unknown-linux-musl --sysroot=...` | musl sysroot from `card/userland/components/musl.md` |
| `-march=x86-64` plus the `-mno-*` list | The deletion list. Each `-mno-` corresponds to a row in `docs/research/isa-deletions.md`. |
| `-Xclang -target-feature -Xclang -cmov` | No `-mno-cmov` driver flag exists; the backend feature is passed directly. Effective only with the patched LLVM. |
| `-Xclang -target-feature -Xclang -nopl` | Single-byte NOP padding (patched LLVM honors it in 64-bit mode); the `0F 1F` multi-byte NOP is unverified on KNC. |
| `-fcf-protection=none` | No `endbr64` (CET is absent; the encoding is unverified on KNC). |
| `-fno-jump-tables` | Jump tables are fine for the ISA; this keeps `.text` free of embedded data so `phi-isa-audit`'s linear sweep does not misdecode. Small code-size cost. |
| `-fuse-ld=lld` | The LLVM linker from the same install. |

`PHI_LLVM` and `PHI_SYSROOT` override the default locations under
`toolchain/build/`. The wrapper refuses to run with an unpatched clang
found elsewhere, because that clang would silently emit CMOV.

## Linking flag

`-fuse-ld=lld` is passed only when the invocation links (no `-c`, `-S`,
`-E`, `-M`, `--version` and the like among the arguments). clang otherwise
reports it as an unused argument, and the kernel build compiles with
`-Werror=unused-command-line-argument` and probes flags with
`$(CC) -Werror ... -c`, so that note would fail every probe (measured
2026-09-13 while wiring the wrapper in as the kernel's `CC`).

## `-mno-cmov -mno-nopl`

Both are driver flags added by LLVM patch 0009. The earlier form,
`-Xclang -target-feature -Xclang -cmov`, reached only the C compiler
(cc1); assembly files go through the integrated assembler (cc1as), which
`-Xclang` does not reach, so `.p2align` padding in `.S` files was still
emitted as `0F 1F` multi-byte NOPs (measured in the kernel's
`memcpy_64.o`, 2026-09-13). A `-m` feature flag is translated to
`-target-feature` for both jobs.
