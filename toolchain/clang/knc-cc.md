# knc-cc

Driver wrapper so that every C/C++ build for the card gets the same flags.
`CC=knc-cc` is all a project's build system needs.

| Flag | Reason |
| --- | --- |
| `--target=x86_64-unknown-linux-musl --sysroot=...` | musl sysroot from `card/userland/components/musl.md` |
| `-march=x86-64` plus the `-mno-*` list | The deletion list. Each `-mno-` corresponds to a row in `docs/research/isa-deletions.md`. |
| `-Xclang -target-feature -Xclang -cmov` | No `-mno-cmov` driver flag exists; the backend feature is passed directly. Effective only with the patched LLVM. |
| `-fcf-protection=none` | No `endbr64` (CET is absent; the encoding is unverified on KNC). |
| `-fno-jump-tables` | Jump tables are fine for the ISA; this keeps `.text` free of embedded data so `phi-isa-audit`'s linear sweep does not misdecode. Small code-size cost. |
| `-fuse-ld=lld` | The LLVM linker from the same install. |

`PHI_LLVM` and `PHI_SYSROOT` override the default locations under
`toolchain/build/`. The wrapper refuses to run with an unpatched clang
found elsewhere, because that clang would silently emit CMOV.
