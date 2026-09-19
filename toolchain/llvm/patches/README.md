# toolchain/llvm/patches

Nine patches against `llvmorg-22.1.8` (commit `ca7933e4` in `SERIES`),
applied by `toolchain/llvm/build.sh` before it configures. Named
`NNNN-<summary>.patch`, generated with `git format-patch`, each carrying
the Apache-2.0-with-LLVM-exception license of the code it modifies.

`toolchain/llvm/README.md` explains what each one does and why. In short:
0001, 0005 and 0009 remove the assumption that x86-64 implies CMOV; 0002
and 0004 are the knc64-x87 ABI (scalar floats returned in `ST0`, `va_arg`
taking the SSE class from the overflow area); 0003 and 0009 make `-nopl`
reach 64-bit NOP padding; 0006 stops `rep bsf` being emitted without BMI;
0007 builds the compiler-rt x86 builtins without SSE; 0008 turns the
`pause` intrinsic into a `nop`.

Applying them by hand is not part of any workflow: `build.sh` does it and
leaves the tree at the patched state, so a rerun after a clone is
idempotent.
