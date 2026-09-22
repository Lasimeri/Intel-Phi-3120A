# Running unmodified AVX-512 binaries: what is possible and what is not

The goal this note tests: a program compiled for AVX-512, not modified in
any way, runs on this host and gets the card's help. Everything below is
measured on the actual machine (Ryzen 7 5800X, no AVX-512), 2026-09-21.

The answer splits cleanly in two, and the two halves have very different
outlooks.

**Execution transparency is achievable and cheap.** An unmodified AVX-512
binary can be made to run, at about 90 percent of the speed the host would
have reached with native AVX2.

**Detection transparency is not, on this CPU, and is partly
counterproductive even where it works.** The reasons are below, because
they are worth knowing before anyone spends time on it.

## The three facts that decide the design

### CPUID cannot be made to fault on this host

The clean way to advertise a feature the CPU lacks is to make `CPUID` trap
and answer it in a handler. Linux exposes this as
`arch_prctl(ARCH_SET_CPUID, ARCH_CPUID_SIGSEGV)`.

```
ARCH_GET_CPUID -> 1 (ok)
ARCH_SET_CPUID(SIGSEGV) -> -1 (No such device)
```

It returns `ENODEV`, and `/proc/cpuinfo` carries no `cpuid_fault` flag.
CPUID faulting is an Intel feature, bit 0 of `MSR_MISC_FEATURES_ENABLES`,
and Zen 3 does not implement it. So there is no in-process, kernel-assisted
way to tell software this machine has AVX-512.

### The fault an AVX-512 instruction raises is usable

Executing `vaddps zmm0, zmm1, zmm2` on this host:

```
SIGILL: si_code=2 (ILL_ILLOPN) si_addr=0x7f503ecab000 saved RIP=0x7f503ecab000
```

All three properties a trap-and-translate layer needs hold: the fault is
synchronous, the saved `RIP` points **at** the faulting instruction rather
than past it, and a handler can change `RIP` and resume with nothing
corrupted.

### A trap costs 1909 ns, so it must happen once per site and never again

Measured over 200000 traps: **1909 ns** each. A native AVX-512 instruction
is about 1 ns, so trapping on every instruction is a factor of 1900, which
is roughly twenty times worse than Intel SDE.

What makes this survivable is an encoding detail. An EVEX instruction is at
minimum 6 bytes: the `62H` prefix and three payload bytes, an opcode, and a
ModRM byte. A near `jmp rel32` is 5. **So a translated site can always be
overwritten in place with a jump to its translation**, and every site pays
the 1909 ns exactly once, on first execution.

## What transparency costs once the trap is gone

The host has no `zmm` registers at all, so a layer executing AVX-512 on its
behalf has to keep the program's 32 vector registers in memory, a shadow
register file, and do the arithmetic as pairs of 256-bit AVX2 operations.

The question is what that costs. Measured on a degree-30 Horner evaluation,
single thread, with instruction-level parallelism matched across variants so
the comparison isolates the shadow file rather than the unrolling:

| | GFLOP/s | against native |
| --- | --- | --- |
| native AVX2, 8 lanes, one chain | 43.26 | |
| native AVX2, 16 lanes, two chains | **86.18** | 1.00x |
| region translated over the shadow file | **77.38** | 1.11x slower |
| translated one instruction at a time | 13.18 | 6.54x slower |

**Region translation costs 11 percent.** Translating one instruction at a
time costs 6.5x, because every instruction reads both sources from the
shadow file and writes its result back, and nothing is carried in a register
between separately patched sites.

The whole argument for region translation is the gap between those two rows.

One thing worth noting from the first, unmatched run: splitting a 512-bit
operation into two 256-bit ones **doubles the instruction-level parallelism
of a dependent chain**, which on a latency-bound kernel is a speedup. The
first measurement showed the translated version beating native, and that was
this effect rather than a translation win, which is why the baseline above
is the 16-lane one.

## The shape this gives the layer

1. First execution of an AVX-512 site faults. One trap, 1909 ns.
2. The handler decodes forward to find the region, ideally a whole loop
   body, and translates it once.
3. The site is patched with a jump to the translation. It never faults
   again.
4. The translation runs either on the host, AVX2 over the shadow file at
   about 0.9x native AVX2, or on the card for loops whose arithmetic
   intensity justifies the trip, where 57 vector units reach 313 to 412
   GFLOP/s (`docs/results/2026-09-21-avx512-translation.md`).

The host path is the correctness path and is always available. The card is
an accelerator for the cases that suit it, not a dependency.

## Why detection is the harder half, and why succeeding at it can hurt

With CPUID faulting unavailable, the remaining routes each cover one
consumer:

| route | reaches | misses |
| --- | --- | --- |
| `/proc/cpuinfo` replaced in a mount namespace | things that parse it (some runtimes, some installers) | everything that executes `cpuid` directly |
| `GLIBC_TUNABLES=glibc.cpu.hwcaps` | glibc's own string and math dispatch | every other library |
| rewriting `cpuid` sites in the loaded binary | that binary | correctness risk, and the results still have to be synthesised |

None of them is general, because hand-vectorised libraries inline `cpuid`
and check `XCR0` with `xgetbv`, and `XCR0` cannot report vector state the
hardware does not have.

There is also a trap in succeeding. **If AVX-512 is advertised, glibc will
dispatch `memcpy`, `strlen` and friends onto AVX-512 paths**, and those are
short, hot, and called constantly. Each becomes either a trap or a shadow
file round trip, on exactly the operations the system does most. Advertising
the feature without a fast path for those cases makes the machine slower at
its most common work.

So the honest position: execution transparency is worth building, and
detection should be opt-in per program rather than system-wide, applied to
the specific workload someone wants on the card.

## What is not built yet

Everything in "the shape this gives the layer". This note establishes that
the approach is sound and what each piece costs; the trap handler, the
region finder, the patcher and the shadow file do not exist. The translator
that turns EVEX into card instructions does
(`host/crates/avx512-xlate`), and is what the card half would call.
