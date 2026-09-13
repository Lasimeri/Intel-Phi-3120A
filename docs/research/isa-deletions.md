# Instruction-set deletions on Knights Corner

Primary source: Intel Xeon Phi Coprocessor Instruction Set Architecture
Reference Manual, document 327364-001 (September 2012), Appendix B "64 bit
Mode Scalar Instruction Support". Text extracted with `pdftotext` from the
PDF at intel.com (`327364001en.pdf`). Secondary: SSDG 328207-002 section 4.2.

## B.2: explicitly unsupported in 64-bit mode

Quoted from the "GPR and X87 Instructions Not Supported" table:

```
CMOV       CMPXCHG16B   FCMOVcc   FCOMI
FCOMIP     FUCOMI       FUCOMIP   IN
INS        INSB         INSD      INSW
MONITOR    MWAIT        OUT       OUTS
OUTSB      OUTSD        OUTSW     PAUSE
SYSENTER   SYSEXIT
```

Plus, as categories: every instruction that operates on MMX, XMM, or YMM
registers.

## Absent from the B.1 supported table (treated as unsupported)

The supported table is exhaustive by construction. These are not in it and are
corroborated elsewhere:

| Instruction | Corroboration |
| --- | --- |
| `LFENCE`, `MFENCE`, `SFENCE` | Intel community thread "Are there any instructions in k1om can replace lfence"; users had to strip fences from source to build for KNC |
| `PREFETCHT0/1/2/NTA`, `PREFETCHW` | SSDG 4.2.12: "the PREFETCH instruction is not supported", replaced by `VPREFETCH*` |
| `CLFLUSH`, `CLFLUSHOPT` | CPUID leaf 1 EDX bit 19 (CLFSH) = 0 in the ISA manual's CPUID appendix; KNC has `CLEVICT0/1` instead |
| `RDTSCP` | Not listed; MPSS kernel never used it |
| `POPCNT`, `LZCNT`, `TZCNT`, `BMI*`, `MOVBE` | Not listed; all post-P5 additions the core lacks |
| `XSAVE*`, `XGETBV`, `XSETBV` | Not listed; FXSAVE is the only save mechanism |
| Multi-byte `NOP` (`0F 1F /0`) | P6-era encoding; treat as suspect until measured. Compilers emit it for alignment padding. |

## Explicitly supported (so not a problem)

`FXSAVE`, `FXRSTOR` (appendix B.4/B.5, with the KNC note that no zmm state is
saved), `SYSCALL`, `SYSRET`, `SWAPGS`, `RDMSR`, `WRMSR`, `RDTSC`, `RDPMC`,
`XADD`, `CMPXCHG` (8-byte), `LOCK` prefix, full x87, `INVLPG`, `WBINVD`,
`INVD`, `HLT`, `MOV CR/DR`, `LGDT/LIDT/LTR`, `IRET/IRETQ`, `PUSHFQ/POPFQ`,
`MOVSXD`, `SHLD/SHRD`, all `Jcc`/`SETcc`, `LOOP*`, string ops with `REP`.

## CPUID as the card reports it

From the ISA manual CPUID appendix (leaf 1 and 0x80000001):

| Feature | Bit | Value |
| --- | --- | --- |
| FPU, TSC, MSR, PAE, CX8, APIC, PGE reported | various | 1 (but see PGE note in os-limitations.md) |
| CMOV | EDX[15] | 0 |
| CLFSH | EDX[19] | 0 |
| FXSR | EDX[24] | 1 |
| MMX, SSE, SSE2 | EDX[23,25,26] | 0 |
| MONITOR | ECX[3] | 0 |
| CX16 | ECX[13] | 0 |
| SYSCALL (64-bit) | 0x80000001 EDX[11] | 1 |
| Max basic leaf | | 4 |
| Max extended leaf | | 0x80000008 |
| Family / model | | 0x0B / 0x01 |

## What this breaks, by layer

| Layer | Breakage | Mitigation in this project |
| --- | --- | --- |
| Any x86-64 compiler output | `cmov` is part of the x86-64 baseline; gcc 16 and clang 22 emit it and offer no switch in 64-bit mode (measured on this host, see abi-and-toolchain.md) | Patched LLVM honors `-cmov` in 64-bit mode; `phi-isa-audit` verifies every binary |
| Any x86-64 float ABI | float/double arguments and returns go through XMM | Custom ABI: arguments on the stack, return in `ST0`; see decisions/0002 |
| Linux kernel | `cpu_relax()` is `pause`; `mb()/rmb()/wmb()` are fences on 64-bit; `prefetcht0` in `asm/processor.h`; `clflush` in cache-range helpers; `outb`/`inb` in legacy device probing and `io_delay`; `verify_cpu` requires SSE2; `X86_REQUIRED_FEATURE_CMOV` | KNC platform patches; see card/kernel/README.md |
| musl | x86_64 asm uses SSE in `memcpy`/`memset` variants and math; the psABI classification puts floats in XMM | musl configured with the KNC ABI compiler; asm files replaced with C |
| Rust `std` | `x86_64-unknown-linux-musl` assumes SSE2 | Custom target JSON with SSE disabled and `build-std` |
| JIT compilers (V8, JSC/Bun, LuaJIT) | Emit SSE and CMOV at runtime | Excluded; interpreters only |

## KNC-specific replacements worth knowing

`DELAY r32` (in place of `PAUSE`), `CLEVICT0/1 m` (evict a line from L1/L2),
`VPREFETCH0/1/2/NTA` and `VPREFETCHE*`, `SPFLT`, `LZCNT` is absent but
`VPLZCNT` exists in vector form. None are needed for a kernel or scalar
userland; they matter for performance work later.
