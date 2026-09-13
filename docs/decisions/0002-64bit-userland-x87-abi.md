# 0002: Card userland is 64-bit with the knc64-x87 ABI from a patched LLVM

Status: accepted, 2026-09-13.

## Context

- KNC has no XMM registers, so the x86-64 SysV float ABI cannot be used.
- SSDG 4.2.3: no 32-bit compatibility submode. A 32-bit userland (which
  would have used x87 and needed no patches) is impossible.
- Intel's k1om psABI passes floats in zmm registers; only Intel's dead GCC
  4.7/5.1 fork implements it, and that fork cannot build a current kernel.
- Measured: gcc 16 and clang 22 both refuse float returns without SSE and
  both emit CMOV in 64-bit mode with no switch. LLVM already passes float
  arguments on the stack when SSE is off.

## Decision

Define knc64-x87: x86-64 SysV with float/double arguments on the stack and
float/double returns in x87 `ST0`; CMOV disabled. Implement it as a small
LLVM patch (return convention, CMOV implication) used by both clang and
rustc. All card code, C and Rust, is compiled with that LLVM.

## Consequences

- One LLVM build is a project prerequisite (`toolchain/llvm/`).
- Binary compatibility with Intel k1om binaries is not a goal and not
  possible (different float ABI). `e_machine` stays `EM_X86_64`.
- `libffi`, `tcc`, and `gcc` need matching small ABI patches when their turn
  comes.
- Hardware float via x87, not soft-float. Fast enough for scalar code.

## Alternatives rejected

- Intel k1om GCC + zmm ABI: cannot build the kernel; no Rust.
- Soft-float everywhere (no LLVM patch): Rust `std` and musl `libm` would
  disagree on float passing at the FFI boundary unless every C library was
  also soft-float, and clang refuses `-msoft-float` returns on x86-64 anyway
  (measured).
- New GCC backend for k1om: months of work for no gain over the LLVM patch.
