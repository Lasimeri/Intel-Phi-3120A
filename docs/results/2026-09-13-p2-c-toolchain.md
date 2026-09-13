# 2026-09-13: card C toolchain end to end (phase P2, C half)

Host: Arch/CachyOS, 16 threads. Toolchain: patched LLVM 22.1.8 (patches
0001 to 0007), musl 1.2.5 with the XMM/deleted-mnemonic drop rule and the
`a_spin` patch, compiler-rt builtins with `COMPILER_RT_X86_NO_SSE`.

`toolchain/check/run.sh`:

```
libc.a: 1838 executable section(s), 111335 instructions
result: 0 illegal, 0 suspect

hello: 3 executable section(s) [.text, .init, .fini], 9318 instructions
result: 0 illegal, 0 suspect
== run on host
scale=13.5 halve=2.5 bigsum=3 va=7 pick=10 sqrt=1.41421 malloc ok thread=41
rust_hypot=-1
== objdump: float return path
<scale>:
  fldl   0x8(%rsp)
  fmull  0x10(%rsp)
  fadds  -0x1f3a(%rip)
  retq
phase P2 check: PASS
```

## Readings

- Every construct in `hello.c` (`toolchain/check/hello.md`) behaves: float
  and double through the stack/x87 ABI, `va_arg(double)` from the overflow
  area (patch 0004), a 64-bit select without CMOV (patches 0001, 0005),
  `long double`, musl's `printf`, `malloc`, `pthread`.
- The binary runs on the host unchanged: a knc64-x87 program is a plain
  x86-64 ELF that only avoids instructions. The card userland can be
  developed and tested here before the card boots.
- `rust_hypot=-1` is the weak C stub; the Rust half waits for the dylib
  LLVM build and `rust-src`.
- Residues found and removed along the way, each now a reproducible rule:
  `tzcnt` from LLVM's REP-BSF hack (patch 0006), `fucomip` in musl's
  `exp2l.s` (drop rule), `pause` in `a_spin` (musl patch 0001), SSE
  conversion asm and `xgetbv` in compiler-rt (patch 0007), and
  `ldmxcsr`/`stmxcsr` false positives (audit allowlist, ISA App. B.3/B.7).
