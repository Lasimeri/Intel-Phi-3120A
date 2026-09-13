# phi-isa-audit / lib.rs

The engine behind the `phi-isa-audit` binary. It answers one question for a
compiled x86-64 ELF: which instructions in it would raise `#UD` on Knights
Corner?

## Method

1. Parse the ELF with `object`; refuse anything that is not x86-64.
2. For every section of kind `Text`, decode linearly with iced-x86 in
   64-bit mode. Linear sweep is adequate for compiler output; data embedded
   in code (jump tables in `.text`, rare with modern compilers) can produce
   spurious decodes, which is why every hit carries its address and
   disassembly for a human to check.
3. Classify by iced-x86's `cpuid_features()` (the feature bit that gates the
   instruction) against the deletion list, plus mnemonic rules for base-ISA
   instructions KNC deletes anyway (`IN`/`OUT`, `SYSENTER`, `FCMOV`,
   `FCOMI`).

## The lists

`classify_feature` names features as the ISA documents name them. Illegal:
CMOV, all SIMD families, CMPXCHG16B, MONITOR, PAUSE, PREFETCHW, CLFSH,
RDTSCP, MOVBE, POPCNT/LZCNT/BMI, XSAVE family, and every later extension.
Suspect: `MULTIBYTENOP` (the `0F 1F` NOP forms compilers emit for alignment)
and `CET_IBT` (`endbr64`, a NOP on CPUs without CET, emitted by
`-fcf-protection`). Both are P6-era encodings not listed either way (see
`isa-deletions.md`); the card toolchain avoids them with `-fcf-protection=none`
and single-byte NOP padding until they are measured.

`FXSR` is deliberately *allowed*: KNC supports FXSAVE/FXRSTOR (ISA App. B).
`X64`, `TSC`, `MSR`, `CX8`, `INVLPG`, `WBINVD`, `SYSCALL`, and the base
`INTEL386`/`INTEL486` families are allowed.

## Testing

A synthetic byte string with one of each offender plus a clean `nop; ret`.
The binary's integration test runs on `/usr/bin/ls` and expects hits, since
any distro binary is full of CMOV and SSE.
