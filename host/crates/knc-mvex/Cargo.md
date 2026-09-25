# knc-mvex

Encoder for a subset of the Knights Corner vector instruction set (the
MVEX-prefixed 512-bit instructions) and the `knc-mvex-gen` binary that
writes the project's hand-vectorised assembly and the kernel's vector
state header from it. No dependencies. See [`src/lib.md`](src/lib.md),
[`src/conv.md`](src/conv.md), [`src/transc.md`](src/transc.md) and
[`src/main.md`](src/main.md). The library (not the binary) is also in
Intel-Phi-AVX512, kept identical (`src/lib.md`, "The copy in
Intel-Phi-AVX512").
