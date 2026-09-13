# toolchain/llvm

One LLVM (for clang and rustc) with two behavioral changes. The patch files
are produced in phase P2 against a pinned release (`llvmorg-22.1.x`, the
version Arch ships, so the host's own clang can build it).

## Change 1: return float/double in x87 ST0 when SSE is disabled

Measured on the host (`docs/research/abi-and-toolchain.md`): with
`-mno-sse`, clang already compiles argument passing and arithmetic to x87
and only fails on the return. LLVM's calling-convention tables already
express the argument side ("In the case of SSE disabled --> save to stack"
in `llvm/lib/Target/X86/X86CallingConv.td`).

Edits:

1. `X86CallingConv.td`, `RetCC_X86_64_C`: before the rules that assign
   `f32`/`f64` to `XMM0/XMM1`, add
   `CCIfType<[f32, f64], CCIfNotSubtarget<"hasSSE1()", CCAssignToReg<[FP0]>>>`.
   `RetCC_X86_32_C` does exactly this for the i386 ABI and is the template.
2. `X86ISelLowering.cpp`: the `errorUnsupported(..., "SSE register return
   with SSE disabled")` path in `LowerReturn`/`LowerCallResult` becomes
   conditional on `!Subtarget.hasX87()`.
3. clang: `clang/lib/CodeGen/Targets/X86.cpp` (`X86_64ABIInfo`) classifies
   `float`/`double` as SSE class and, when the `sse` feature is absent,
   reports the same diagnostic (`err_sse_reg_return`?). It must instead emit
   the IR type unchanged and let the backend place it; the diagnostic is
   removed for targets with `x87`. The exact site is found by grepping for
   the message text.

## Change 2: 64-bit mode must not imply CMOV

Measured: `-Xclang -target-feature -Xclang -cmov` still emits `cmov` in
64-bit code. In `llvm/lib/Target/X86/X86.td`, `FeatureX86_64` implies
`FeatureCMOV`; remove the implication and audit
`X86Subtarget::canUseCMOV()` users (select lowering falls back to branches
when it returns false, which is the 32-bit `pentium` behavior). Verify with
`phi-isa-audit` on a compiled `select`-heavy file.

## Optional change 3: NOP padding

If phase P2's on-card test shows `0F 1F` multi-byte NOPs fault, use the MC
option that emits single-byte NOP runs (`X86AsmBackend` has a
`Pad-For-Align` policy; the legacy path for CPUs without long NOPs exists
because `pentium` lacks them). Prefer the flag to a patch.

## Build recipe

```
cmake -S llvm -B build -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DLLVM_ENABLE_PROJECTS="clang;lld" \
  -DLLVM_TARGETS_TO_BUILD=X86 \
  -DLLVM_ENABLE_RUNTIMES="compiler-rt;libunwind;libcxxabi;libcxx" \
  -DLLVM_DEFAULT_TARGET_TRIPLE=x86_64-unknown-linux-musl \
  -DCMAKE_INSTALL_PREFIX=$PWD/../build/llvm
ninja -C build install
```

Runtimes are built later against the musl sysroot with `knc-cc`; the first
pass builds only the compilers. Expect one to two hours on the 5800X.

## rustc

rustc is built with `llvm-config` pointing at this install
(`[target.x86_64-unknown-linux-gnu] llvm-config = ...` in `bootstrap.toml`)
so that the custom target's codegen is the patched one. Alternatively
`-Zbuild-std` with a stock nightly plus `+soft-float` is a fallback that
loses FFI float compatibility; `docs/decisions/0002` explains why it is not
the default.
