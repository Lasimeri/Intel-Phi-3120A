# card/kernel

A mainline Linux kernel for Knights Corner: a small patch series plus a
config fragment, built with the project's patched clang.

## Patch series (to be written in phase P3 against a pinned tag)

Ordered so that each patch is reviewable on its own and maps to a row of
SSDG section 4.2 (`docs/research/os-limitations.md`) or to a measured
instruction-set fact (`docs/research/isa-deletions.md`).

| # | Patch | Files | Why |
| --- | --- | --- | --- |
| 1 | `x86: add CONFIG_X86_KNC platform option` | `arch/x86/Kconfig` | Umbrella symbol; selects `X86_EXTENDED_PLATFORM`, `X86_LOCAL_APIC`, `X86_IO_APIC`; depends on `!X86_KERNEL_IBT`, `!MITIGATION_RETPOLINE`, `!KVM`, `!X86_MCE`, `!IA32_EMULATION` |
| 2 | `x86/cpufeatures: relax required features for KNC` | `arch/x86/Kconfig.cpufeatures`, `arch/x86/kernel/verify_cpu.S` | `XMM`, `XMM2`, `CMOV`, `PGE` no longer required when `X86_KNC`; `verify_cpu` skips the SSE test (SSDG 4.2.1, 4.2.10; ISA App. B) |
| 3 | `x86/barrier: lock-prefixed barriers for KNC` | `arch/x86/include/asm/barrier.h` | `mfence`/`lfence`/`sfence` are absent; use `lock addl $0,-4(%rsp)` for all three, as the 32-bit build already does when `XMM2` is missing |
| 4 | `x86: cpu_relax and prefetch without PAUSE and PREFETCH` | `arch/x86/include/asm/vdso/processor.h`, `asm/processor.h` | `PAUSE` is unsupported (App. B.2); `PREFETCH` is unsupported (SSDG 4.2.12). `cpu_relax` becomes a plain barrier (or KNC `delay`), `prefetch*` become no-ops |
| 5 | `x86: no port I/O on KNC` | `arch/x86/include/asm/io.h`, `arch/x86/kernel/io_delay.c`, `arch/x86/kernel/reboot.c` | `IN`/`OUT` are unsupported; `io_delay` becomes a no-op, `inb`/`outb` become `WARN_ONCE` stubs, reboot goes through SBOX |
| 6 | `x86/knc: platform layer` | `arch/x86/kernel/knc.c` (new), `arch/x86/kernel/setup.c` hook | `x86_init` and `x86_platform` overrides modeled on `jailhouse.c`: no legacy PIC/PIT/RTC/i8042, I/O APIC at its 64-bit address, LAPIC timer calibrated from the SBOX frequency register, wall clock from the ring header, `ARAT` set, `PGE` cleared, POST codes written at milestones (SSDG 4.2.2, 4.2.4, 4.2.5, 4.2.6, 4.2.17.1) |
| 7 | `x86/knc: minimal SFI parser` | `arch/x86/kernel/knc_sfi.c` (new) | The bootstrap reports CPUs and memory via SFI 0.7 tables (SSDG 2.2.4.2); mainline removed SFI in 5.12. Parses `SYST`, `CPUS`, `MMAP`, `APIC` only |
| 8 | `x86/knc: ring early console` | `arch/x86/kernel/knc_earlycon.c` (new) | `earlycon`/`console=phiring`: writes into the card-to-host console ring located by `phi.ring=` (`docs/spec/ring-protocol.md`) |
| 9 | `x86/knc: cache flush helpers` | `arch/x86/include/asm/special_insns.h`, `arch/x86/mm/pat/set_memory.c` | `CLFLUSH` is unsupported (CPUID CLFSH = 0); `clflush_cache_range` falls back to `wbinvd` or KNC `clevict` |
| 10 | `x86/knc: SMP bring-up` | `arch/x86/kernel/smpboot.c` | Whatever the APs parked by the bootstrap need beyond INIT/SIPI; determined by comparing with Intel's `intelmic.c`/`smpboot.c` (risk register in `docs/plan.md`) |

Patches 1 to 5 and 9 are mechanical. 6 to 8 are new code (drafts in
`platform/`). 10 is the unknown.

## Build

```sh
# From a pinned mainline tag, with the patched LLVM in PATH first:
make LLVM=1 KCFLAGS="-Xclang -target-feature -Xclang -cmov -fcf-protection=none" \
     defconfig
scripts/kconfig/merge_config.sh .config ../../card/kernel/config/knc.config
make LLVM=1 KCFLAGS="..." -j$(nproc) bzImage
phi-isa-audit vmlinux            # must report 0 illegal
```

`vmlinux` contains alternative-instruction replacement slots in a separate
section that the audit does not scan; `.text` must be clean.

## Boot path recap

32-bit entry (`startup_32` in `arch/x86/boot/compressed/head_64.S`) with
`%esi` = the bootstrap's `boot_params`. Since Linux 5.15 the decompressor
loads its own GDT immediately, so the bootstrap's unusual descriptor order
(SSDG 2.2.4.2) does not matter. The decompressor's own `verify_cpu` call is
patch 2's other target.

## Confirmed sites in v7.2.3 (inspected 2026-09-13, tree in `build/linux`)

| Patch | Site | Finding |
| --- | --- | --- |
| 1, 6 | `arch/x86/kernel/cpu/hypervisor.c` `hypervisors[]`, `arch/x86/kernel/jailhouse.c` | The hook is the hypervisor table: `x86_hyper_jailhouse` provides `.detect` and `.init.init_platform`, called from `init_hypervisor_platform()` in `setup_arch` before timers and APIC setup. `x86_hyper_knc` does the same with `detect` = CPUID family 0x0B model 0x01. |
| 2 | `arch/x86/Kconfig.cpufeatures` lines 48 to 99 | `CMOV` (via `X86_CMOV`), `PSE`, `PGE`, `FXSR`, `XMM`, `XMM2` are required on `X86_64`. `verify_cpu.S` builds its `SSE_MASK` from `REQUIRED_MASK0`, so once `XMM`/`XMM2` are not required the decompressor's SSE test passes without touching assembly. `FXSR` stays required (KNC has it). |
| 2 | `arch/x86/kernel/head_64.S:232` | `btsl $X86_CR4_PGE_BIT, %ecx` unconditionally on the way to 64-bit mode for the boot CPU and every AP. SSDG 4.2.10: this write is a #GP on KNC. Guarded out under `CONFIG_X86_KNC`. `arch/x86/mm/init.c:243` sets PGE only behind the feature bit, which patch 6 clears. |
| 4 | `arch/x86/include/asm/vdso/processor.h:13` | `native_pause()` is a bare `pause`; becomes a compiler barrier under KNC. |
| 4 | `arch/x86/include/asm/processor.h:621` | `BASE_PREFETCH` is `prefetcht0` on 64-bit, `prefetchnta` and `prefetchw` via alternatives; all become empty under KNC. |
| 9 | `arch/x86/include/asm/special_insns.h:185-203` | `clflush()`, `clflushopt()`, `clwb()` are the only inline sites; `clflush_cache_range` in `mm/pat/set_memory.c` builds on them. Under KNC: `wbinvd` fallback. |
| 6 | `arch/x86/kernel/apic/apic.c:743` | `calibrate_APIC_clock` returns early when `lapic_timer_period` is preset, exactly the jailhouse trick; the platform layer sets it from the core frequency. |
| risk | `arch/x86/kernel/cpu/intel.c` | `early_init_intel` reads `MSR_IA32_MISC_ENABLE` through the `_safe` helpers (no fault on a missing MSR), but `init_intel` later uses unguarded `rdmsrq` on family checks. KNC is family 0x0B, so family-6 paths are skipped; the rest is verified against Intel's tree once `vendor/` is populated. |
| n/a | `arch/x86/boot/compressed/head_64.S:168` | The decompressor sets only `CR4.PAE` at entry; no PGE there. |
