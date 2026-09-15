# Runtime feasibility on the card

Everything on the card is 64-bit, uses the knc64-x87 ABI, links against musl,
and must pass `phi-isa-audit`.

| Runtime | Verdict | Basis | Plan |
| --- | --- | --- | --- |
| **Rust** userland | Feasible | Custom target with SSE and CMOV disabled; `build-std` on nightly; LLVM x87 codegen exists | Card `init`, ring endpoints, service supervisor, and tools are Rust. |
| **C toolchain on the card: clang** | Feasible, large binary | clang is C++ with no ISA-specific code; needs libc++ built for musl/knc | First native compiler on the card. |
| **gcc** on the card | Feasible with a small gcc backend patch | gcc needs the same two amendments (no CMOV in 64-bit, x87 return): `i386.cc` `function_value_64` currently errors "SSE register return with SSE disabled"; add a target option to route it to `FIRST_FLOAT_REG`; add `-mno-cmov` gating for `TARGET_CMOV` in 64-bit | Built as a cross first, then Canadian-cross to run natively. Phase P8. |
| **tcc** on the card | Feasible with backend work | `x86_64-gen.c` uses SSE (`movsd`, `xmm` registers) for all float codegen and the SysV register ABI. The i386 backend already has an x87 code path to borrow from. Estimated at a few hundred lines. tcc itself emits no CMOV. | Phase P8. Run on the card natively; `tcc -run` for scratch programs. |
| **QuickJS** (or QuickJS-ng) | Feasible | Pure C99, interpreter only, no JIT, doubles via the C ABI | The JavaScript runtime. Phase P9. |
| **Bun** | **Impossible** | Bun documents SSE4.2 as a hard requirement even in `x64-baseline` builds and crashes with "Illegal instruction" without it; JavaScriptCore JITs SSE code; Zig/LLVM cannot target KNC vectors | Replaced by QuickJS. See decisions/0004. |
| **Node.js** | Impossible in practice | V8 requires SSE2 on ia32 and x64 | Not attempted. |
| **CPython 3.13+** | Feasible | Standard cross-build (`--with-build-python`); prior k1om builds of 2.7 and 3.4 exist; `_ctypes` needs a libffi patch for the float ABI (x87 return, stack args) analogous to the 2013 zmm patch; `_decimal`, `_hashlib` (OpenSSL) are plain C | Phase P9, with `ensurepip`, `sqlite3`, `ssl`, `zlib`, `bz2`, `lzma`, `ctypes`. |
| **musl** | Feasible | x86_64 port has a handful of SSE-using `.s` files (`memcpy`, `memset`, `fabs`, `sqrt`, `fma`, etc.) replaced by the C generics or x87 versions; classification of floats follows the compiler | Phase P4. |
| **busybox, dropbear** | Feasible | Plain C | Phase P4/P5. |
| **OpenSSH** | Feasible, deferred | OpenSSL has hand-written SSE asm; build with `no-asm` | After dropbear works. |

## Vector unit

None of the runtimes above use the 512-bit VPU: no compiler in the stack
knows the KNC vector ISA (only Intel's dead gcc fork and ICC did). Since
2026-09-15 the project has its own encoder for a subset of it
(`host/crates/knc-mvex`), kernel support for the vector state (patch
0024) and one hand-vectorised kernel, the Mandelbrot loop in
`card/examples/mandel_vpu.S`; see `docs/results/2026-09-15-vpu.md`.
A compiler backend remains out of scope.
