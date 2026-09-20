# zstd

Zstandard (`libzstd` plus the `zstd` driver) for the card, static, with
multithreading. Pinned to the host's version so a card-against-host
measurement compares the same algorithm; 1.5.7 at the time of writing.

Results are in `docs/results/2026-09-19-zstd.md`. Short version: the card
manages 3 to 7% of the host's throughput, far worse than its 22 to 26% on
xz, and **level 3, the default, currently faults with SIGILL**.

## Known defect: levels 3 and 4 fault

```
$ zstd -3 -c file > out
Illegal instruction
```

20 `cmov` instructions survive in `zstd_fast.o` (12) and
`zstd_double_fast.o` (8), which back compression levels 1 to 4. Levels 1,
5, 9 and 19 run correctly; level 3 does not. Use `-1` or `-5` and above
until this is fixed.

They are ordinary compiler output, not assembly and not a target
attribute: a pointer select inside the hot match loop, of the shape
`matchBase = index < prefixStart ? dictBase : base`, compiled to
`cmovaq` between a global address and a computed pointer. That should be
impossible, because LLVM patch 0001 makes `canUseCMOV()` false and patch
0005 adds the `CMOV_GR64` branch-expansion pseudo. What is known:

- It survives `-O2`, `-O1` and `-Os`.
- It survives `-mllvm -x86-cmov-converter=false` and
  `-mllvm --disable-early-ifcvt=true`, so neither of those passes is
  creating it.
- It does **not** reproduce in a minimal case. A standalone
  `return a > b ? g_global : p;` compiles to a branch, correctly. The
  trigger appears to need the register pressure of the real loop.

So it is a gap in the patched LLVM rather than anything zstd does wrong,
and it wants its own investigation: a tenth patch in
`toolchain/llvm/patches/` once the responsible pass is identified. The
audit caught it before it shipped, which is what the audit is for.

## The build trap: target attributes beat the command line

The first build had 727 illegal instructions: 660 `shlx` (BMI2), 27
`lzcnt`, 20 `tzcnt` (BMI1) and 20 `cmov`. None came from
`huf_decompress_amd64.S`, which `ZSTD_DISABLE_ASM` already excluded.

They came from zstd's runtime CPU dispatch. `lib/common/compiler.h`
defines:

```c
#define BMI2_TARGET_ATTRIBUTE TARGET_ATTRIBUTE("lzcnt,bmi,bmi2")
```

and applies it to whole function bodies in `fse_decompress.c`,
`entropy_common.c` and `huf_decompress.c`, choosing between them with a
CPUID check at run time. **A function-level `__attribute__((target(...)))`
overrides the command line**, so `-mno-bmi2` never reaches those bodies,
and because the attribute resets the feature baseline rather than adding
to it, CMOV comes back inside them as well.

`-DDYNAMIC_BMI2=0` removes all 707. The CPUID guard means the card would
never have called them, so this is stricter than strictly necessary, but
this project audits linked executables with no exceptions and dead illegal
instructions in a shipped binary are a trap waiting for the next person.

This is a second, distinct mechanism from the one xz exposed
(`xz.md`: inline assembly guarded on `__x86_64__` as a proxy for "has
CMOV"). Both are the same underlying assumption, that x86-64 implies a
feature set, expressed in different dialects. Anything else ported here
should be checked for both.

## Using it

```sh
card/userland/components/zstd.sh
phi put ~/.cache/intel-phi-3120a-build/userland/zstd/zstd /opt/phi/bin/zstd
```

`init` links `/opt/phi/bin` into `/usr/bin` at boot, so it is on the
default PATH from the next restart. Unlike xz, zstd's Makefile passes
`LDFLAGS` straight to the program link with no libtool in the way, so
`-static` works and no relink fallback is needed.

`libzstd.a` and `zstd.h` go into the card sysroot for anything else built
for the card.

## Tuning

Memory per thread is far smaller than LZMA's, so thread count is not
capped by RAM the way xz's is (`xz.md`). It is capped by bandwidth
instead: level 1 peaked at 114 threads (94 MB/s) and fell back to 78 MB/s
at 228, matching the turnover at two threads per core in
`card/examples/membw.md`. Levels 5 and 9 are flat from 57 threads up.
