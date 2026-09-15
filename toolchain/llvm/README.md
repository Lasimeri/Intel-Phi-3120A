# toolchain/llvm

One LLVM (for clang and rustc) with four small changes, applied by
`build.sh` from `patches/` onto the pinned tag `llvmorg-22.1.8` (the version
Arch ships, so the host clang builds it). What the sources showed on
inspection, versus the design notes written before the tree was cloned:

| Patch | File | Change | What inspection showed |
| --- | --- | --- | --- |
| 0001 | `llvm/lib/Target/X86/X86Subtarget.h` | `canUseCMOV()` no longer returns true for `is64Bit()` | This one line was the whole reason `-cmov` was ignored in 64-bit mode. `FeatureX86_64` implies nothing; every consumer (isel patterns via `HasCMOV`/`NoCMOV`, FastISel, GlobalISel, select lowering) goes through `canUseCMOV()`, but the branch-expansion pseudo for `GR64` did not exist (patch 0005). |
| 0002 | `llvm/lib/Target/X86/X86CallingConv.td` | `RetCC_X86_64_C`: `f32/f64` to `FP0/FP1` when `!hasSSE1()` and `hasX87()`, ahead of the XMM rules | Copied from `RetCC_X86_32_C`. The two "SSE register return with SSE disabled" diagnostics in `X86ISelLoweringCall.cpp` are gated on an XMM register having been *assigned*, so with this rule they never fire and need no edit. The x87 return paths in `LowerReturn`/`LowerCallResult` are not gated on 32-bit mode. Argument passing already goes to the stack when SSE is off. |
| 0003 | `llvm/lib/Target/X86/MCTargetDesc/X86AsmBackend.cpp` | `getMaximumNopSize` returns 1 whenever `nopl` is off, not only in 32-bit mode | The `0F 1F` multi-byte NOP is undocumented on KNC; single-byte padding costs nothing. `knc-cc` and the Rust target pass `-nopl`. |
| 0004 | `clang/lib/CodeGen/Targets/X86.cpp` | `EmitVAArg`: an SSE-class vararg with no INTEGER part reads from the overflow area when the target has no SSE | The callee never spills XMM registers into the register save area (`LowerFormalArguments` skips it without SSE) and `fp_offset` starts at 48, so unpatched `va_arg(ap, double)` would read garbage from the save area. Known limitation: aggregates needing both INTEGER and SSE eightbytes as varargs. |
| 0005 | `llvm/lib/Target/X86/X86InstrCompiler.td`, `X86ISelLowering.cpp` | `CMOV_GR64` branch-expansion pseudo under `NoCMOV`, plus its two dispatch cases | The pseudos existed for `GR8/GR16/GR32` only, because no 64-bit target ever lacked CMOV. Without it a `select i64` fails instruction selection once patch 0001 disables CMOV. |
| 0006 | `llvm/lib/Target/X86/X86MCInstLower.cpp` | The "REP BSF" hack (emit `bsf` with a REP prefix so modern CPUs run it as `tzcnt`) only when the target has BMI | Measured: musl malloc compiled to `tzcnt` with `-bmi`. The hack assumes pre-BMI CPUs ignore the prefix, undocumented for KNC. |
| 0007 | `compiler-rt/lib/builtins/CMakeLists.txt` | New option `COMPILER_RT_X86_NO_SSE`: drops `x86_64/float*` (hand-written SSE conversions) so the generic C versions are used, and drops `cpu_model/x86.c` (`xgetbv` behind a runtime check) | Measured in the first builtins archive audit. Cost: no `__builtin_cpu_supports` on the card. |
| 0008 | `llvm/lib/Target/X86/X86InstrSSE.td`, `X86InstrPredicates.td` | `PAUSE` selects the `llvm.x86.sse2.pause` intrinsic only with SSE2; a codegen-only `PAUSE_NOSSE2` encodes the one-byte `nop` for it otherwise (new `NoSSE2` predicate) | Measured: Rust's `std` linked ten `pause` instructions (futex `Mutex`/`RwLock` spin loops, the stack-overflow handler) through `core::hint::spin_loop`, which LLVM emitted regardless of features because the encoding is a `rep nop` that ordinary pre-SSE2 CPUs ignore. KNC raises #UD on it (ISA App. B). The `pause` mnemonic still assembles. |
| 0009 | `clang/include/clang/Options/Options.td` | New driver flags `-mcmov`/`-mno-cmov` and `-mnopl`/`-mno-nopl` in the x86 feature group | The generic driver code turns any flag in that group into `-target-feature` for both cc1 and cc1as. `-Xclang -target-feature` reached only cc1, so `.S` files (kernel `memcpy_64.S`, `entry_64.S`) were still padded with `0F 1F` NOPs. `knc-cc` now uses the two flags. |

Nothing in clang's frontend diagnoses float returns; the message seen in
the research came from the backend with the function's location attached.

## Card variant (P7)

`PHI_LLVM_VARIANT=card` builds clang, lld and the binutils-style tools for
the card itself: a Canadian cross with `knc-cc`/`knc-c++`, the host
variant's tablegen, static on musl and libc++ (`toolchain/libcxx/build.sh`
first), X86 only, no zlib/zstd/libxml2/terminfo. `build` compiles only the
tools; `install` is a no-op (`card/userland/components/clang.sh` packages
the binaries); `check` audits the card clang and compiles a probe with it
on the host.

## Build recipe

`build.sh` (see `build.md`). Summary:

```
toolchain/llvm/build.sh all      # fetch, patch, configure, build, install, check
```

Runtimes (`compiler-rt`, `libunwind`, `libc++`) are built afterwards with
`knc-cc` against the musl sysroot; they are not part of this step.

## rustc

rustc is built with `llvm-config` pointing at this install
(`[target.x86_64-unknown-linux-gnu] llvm-config = ...` in `bootstrap.toml`)
so that the custom target's codegen is the patched one. Until then, a stock
nightly with `-Zbuild-std` can compile the card target for integer-only
code, but any `f64` return would hit the unpatched diagnostic.
