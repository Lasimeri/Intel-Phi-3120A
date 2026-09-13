# toolchain/llvm

One LLVM (for clang and rustc) with four small changes, applied by
`build.sh` from `patches/` onto the pinned tag `llvmorg-22.1.8` (the version
Arch ships, so the host clang builds it). What the sources showed on
inspection, versus the design notes written before the tree was cloned:

| Patch | File | Change | What inspection showed |
| --- | --- | --- | --- |
| 0001 | `llvm/lib/Target/X86/X86Subtarget.h` | `canUseCMOV()` no longer returns true for `is64Bit()` | This one line was the whole reason `-cmov` was ignored in 64-bit mode. `FeatureX86_64` implies nothing; every consumer (isel patterns via `HasCMOV`/`NoCMOV`, FastISel, GlobalISel, select lowering) goes through `canUseCMOV()`, and the branch-expansion pseudos exist for every GPR class, so no other change is needed. |
| 0002 | `llvm/lib/Target/X86/X86CallingConv.td` | `RetCC_X86_64_C`: `f32/f64` to `FP0/FP1` when `!hasSSE1()` and `hasX87()`, ahead of the XMM rules | Copied from `RetCC_X86_32_C`. The two "SSE register return with SSE disabled" diagnostics in `X86ISelLoweringCall.cpp` are gated on an XMM register having been *assigned*, so with this rule they never fire and need no edit. The x87 return paths in `LowerReturn`/`LowerCallResult` are not gated on 32-bit mode. Argument passing already goes to the stack when SSE is off. |
| 0003 | `llvm/lib/Target/X86/MCTargetDesc/X86AsmBackend.cpp` | `getMaximumNopSize` returns 1 whenever `nopl` is off, not only in 32-bit mode | The `0F 1F` multi-byte NOP is undocumented on KNC; single-byte padding costs nothing. `knc-cc` and the Rust target pass `-nopl`. |
| 0004 | `clang/lib/CodeGen/Targets/X86.cpp` | `EmitVAArg`: an SSE-class vararg with no INTEGER part reads from the overflow area when the target has no SSE | The callee never spills XMM registers into the register save area (`LowerFormalArguments` skips it without SSE) and `fp_offset` starts at 48, so unpatched `va_arg(ap, double)` would read garbage from the save area. Known limitation: aggregates needing both INTEGER and SSE eightbytes as varargs. |

Nothing in clang's frontend diagnoses float returns; the message seen in
the research came from the backend with the function's location attached.

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
