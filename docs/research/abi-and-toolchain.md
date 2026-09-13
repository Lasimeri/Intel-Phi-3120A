# ABI and toolchain

## What Intel did: the k1om psABI

Source: "System V Application Binary Interface, K1OM Architecture Processor
Supplement, Version 1.0" (H.J. Lu, Milind Girkar et al., 2012-04-26), PDF
attached to an Intel community post; text extracted with `pdftotext`.

- Integer arguments as x86-64 SysV: `rdi, rsi, rdx, rcx, r8, r9`.
- **float and double arguments in `zmm0` to `zmm7`**, returns in `zmm0/zmm1`
  (the "SSE class" is mapped onto the 512-bit registers). `%al` carries the
  vector-register count for varargs.
- `long double` in x87 `st0` as usual.
- Syscalls via `syscall`; kernel clobbers `rcx` and `r11`.
- Required processor features (table A.1): `fpu`, `tsc`, `cx8`, `v` (the
  vector unit, "required for float and double"), `syscall`.
- ELF `e_machine` 181 (`EM_K1OM`), `elf64-k1om`, dynamic linker
  `/lib64/ld-linux-k1om.so.2`. Mainline binutils still recognizes the machine
  number (Revival Project observation: host `readelf` prints "Intel K1OM").
- Appendix A.1 says "K1OM processors are able to execute ... 32-bit ia32
  programs". This sentence is inherited verbatim from the x86-64 psABI. The
  SSDG (4.2.3) explicitly denies the compatibility submode. The SSDG wins;
  the psABI chapter 1 itself says the ABI "does not apply to such programs"
  and only "may" be supported.

## Compilers that could emit k1om

| Compiler | Status |
| --- | --- |
| Intel ICC `-mmic` | Proprietary, discontinued. |
| Intel GCC 4.7.0 (MPSS 2.x/3.x) and 5.1.1 (MPSS 3.6+), target `k1om-mpss-linux` / `x86_64-k1om-linux` | Sources shipped with MPSS; a cross-build recipe exists at github.com/apc-llc/gcc-5.1.1-knc. Too old to build a current kernel (minimum GCC is 8.1 per `Documentation/process/changes.rst` on master). |
| Upstream GCC | Never had a k1om target. |
| Upstream LLVM | Never had a k1om target. A 2013 mailing-list patch added scalar-only `k1om64`; it was not merged. |
| Rust | No target. |

## Measured on this host (gcc 16.2.1, clang 22.1.8, 2026-09-13)

```
int f(int a,int b,int c){return c?a:b;}
double g(double a){return a*2.0;}
```

| Command | Result |
| --- | --- |
| `gcc -O2 -S` | emits `cmov` |
| `gcc --help=target` | no option to disable CMOV in 64-bit mode (only `-march` choices, all of which include it) |
| `clang -O2 -Xclang -target-feature -Xclang -cmov` | still emits `cmovel`/`cmoveq` (64-bit mode forces the feature) |
| `gcc -O2 -mno-sse -mno-mmx -mfpmath=387` on `g` | `error: SSE register return with SSE disabled` |
| `clang -O2 -mno-sse -mno-mmx` on `g` | same error; argument and body compile to x87 (`fldl 8(%rsp)`, `fmull`), only the return is rejected |
| `clang -msoft-float -mno-sse` | same error |
| `gcc -m64 -msoft-float -mno-sse` | same error |
| `gcc -m32 -march=i586` | no `cmov`, x87 float, correct in principle, but 32-bit mode is unavailable on the card |

Two facts follow. First, x87 codegen in 64-bit mode already works in LLVM;
only the *return convention* and CMOV forcing block it. Second, no flag-only
solution exists; one small LLVM patch is required.

## The chosen ABI: "knc64-x87"

x86-64 SysV with two amendments, both already half-implemented in LLVM:

1. **Arguments** of class SSE (`float`, `double`) are passed on the stack.
   LLVM does this today when SSE is disabled (`X86CallingConv.td`: "In the
   case of SSE disabled --> save to stack" for `f32, f64, f128`).
2. **Returns** of `float`/`double` go in x87 `ST0`, the same convention the
   i386 ABI uses. This is the LLVM patch: in `RetCC_X86_64_C` add
   `CCIfType<[f32, f64], CCIfNotSubtarget<"hasSSE1()", CCAssignToReg<[FP0]>>>`
   ahead of the XMM rules, and drop the `errorUnsupported("SSE register return
   with SSE disabled")` path in `X86ISelLowering.cpp` when x87 is available.
   Clang's own frontend check, if it fires separately, gets the same
   treatment.
3. **CMOV**: make `FeatureX86_64` stop implying `FeatureCMOV` in `X86.td` and
   audit `canUseCMOV()` users. Verified by `phi-isa-audit` on the output.

`long double`, integers, pointers, structs: unchanged. Vector types never
cross an ABI boundary in this project (no `__m512` arguments).

Both clang and rustc consume the same patched LLVM, so C and Rust agree on
float passing at every FFI boundary, including `libm`.

## Rust target

`toolchain/rust/x86_64-knc-linux-musl.json`: derived from
`x86_64-unknown-linux-musl` with `features` set to
`-mmx,-sse,-sse2,-sse3,-ssse3,-sse4.1,-sse4.2,-avx,-avx2,-cmov,+x87`,
`cpu` set to `x86-64`, and `crt-static` on. Requires nightly for
`-Zbuild-std` because no prebuilt `std` exists. Rust's `std` for musl is
plain Rust plus libc calls, so nothing else needs porting; `compiler-builtins`
provides the math intrinsics.

## Kernel toolchain

The kernel is built with `LLVM=1` using the patched clang. Kbuild already
passes `-mno-sse -mno-mmx -mno-sse2 -mno-3dnow -mno-avx` and
`-mcmodel=kernel -mno-red-zone`. The KNC config adds `-cmov` through
`KCFLAGS`. Hand-written assembly in `arch/x86` is audited the same way.
