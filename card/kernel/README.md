# card/kernel

A mainline Linux kernel for Knights Corner: a sixteen-patch series against
a pinned stable tag (`patches/SERIES`), a Kconfig fragment
(`config/knc.config`), and `build.sh`, which fetches, patches, configures,
builds with the project's patched clang and audits the result.

## The series (v7.2.3)

Sixteen patches. Each patch is a reviewable commit whose message cites the SSDG section
(`docs/research/os-limitations.md`), the ISA reference appendix
(`docs/research/isa-deletions.md`), Intel's card kernel, or a measurement.

| # | Patch | Files | What and why |
| --- | --- | --- | --- |
| 0001 | `x86/Kconfig: add X86_KNC` | `arch/x86/Kconfig` | Umbrella option; depends on `HYPERVISOR_GUEST` (the platform layer uses the hypervisor detection table like jailhouse) and excludes IBT, KVM, MCE, IA32 emulation, retpolines, x2APIC |
| 0002 | `x86/cpufeatures: relax the required-feature set` | `Kconfig.cpufeatures`, `verify_cpu.S`, `head_64.S` | CMOV, PGE, XMM, XMM2 no longer required; `verify_cpu` skips the `IA32_MISC_ENABLE` XD write for family 0xb; `head_64.S` leaves CR4.PGE clear (SSDG 4.2.10) |
| 0003 | `x86/barrier: locked add instead of fences` | `asm/barrier.h` | `mb/rmb/wmb` become `lock addl $0,-4(%rsp)` (ISA App. B.5) |
| 0004 | `x86: no PAUSE and no PREFETCH` | `asm/vdso/processor.h`, `asm/processor.h` | `cpu_relax` is a compiler barrier, `prefetch*` are no-ops (App. B.2, SSDG 4.2.12) |
| 0005 | `x86/io: port I/O reads all ones and writes nothing` | `asm/shared/io.h`, `io_delay.c` | `in*` return ~0, `out*` drop; probes see an empty bus (App. B.4, SSDG 4.2.2) |
| 0006 | `x86: single-byte-safe NOP padding, BSF not TZCNT` | `asm/nops.h`, `asm/bitops.h`, `tools/objtool/arch/x86/decode.c` | Prefixed one-byte NOPs instead of `0F 1F` (undocumented for the P54C-derived core; Intel's kernel used the same), also in objtool's own table since it rewrites jump-label sites at build time; `__ffs` uses `bsf` |
| 0007 | `x86: cache-line flush helpers without CLFLUSH` | `asm/special_insns.h`, `arch/x86/Kconfig` | `clflush/clflushopt/clwb` become `wbinvd` for the few direct callers (CPUID CLFSH is clear so the PAT code already takes the WBINVD path); `ARCH_HAS_UACCESS_FLUSHCACHE` not selected (its `__memcpy_flushcache` uses `movnti`) |
| 0008 | `x86/cpu/intel: skip the MSRs Knights Corner does not implement` | `cpu/intel.c`, `cpu/microcode/core.c` | No `IA32_BIOS_SIGN_ID`, `IA32_PLATFORM_ID`, `IA32_MISC_ENABLE` accesses for family 0xb (Intel's own comment in the k1om tree); no Debug Store probe; early microcode loader disabled; REP_GOOD/ERMS cleared as Intel did |
| 0009 | `x86/ioapic: accept I/O APIC addresses above 4 GiB` | `apic/io_apic.c`, `asm/io_apic.h` | `mp_register_ioapic` takes `phys_addr_t` and keeps the full address next to the MP-table copy (SSDG 4.2.5; Intel widened the same field) |
| 0010 | `x86/knc: minimal SFI table reader` | `asm/knc.h`, `kernel/knc_sfi.c` | SYST search in 0xE0000..0xFFFFF, checksum, CPUS/APIC/MMAP (SSDG 2.2.4.2; layouts from `include/linux/sfi.h` of Linux 5.11); `asm/knc.h` carries the card addresses and the POST codes |
| 0011 | `x86/knc: early console into the shared-memory ring` | `kernel/knc_earlycon.c`, `early_printk.c` | `earlyprintk=phiring[,keep]`, region from `phi.ring=<base>,<size>`, layout per `docs/spec/ring-protocol.md`; sets `card_boot_flags` bit 0 for the host |
| 0012 | `x86/knc: platform layer` | `kernel/knc.c`, `cpu/hypervisor.c`, `asm/hypervisor.h`, `boot/compressed/misc.c` | Hypervisor-table hook detected by CPUID family 0xb model 1; CPUs and I/O APIC from SFI, LAPIC at the default address; no legacy devices; core clock decoded from SBOX SCRATCH4 and CURRENT_CLK_RATIO as the TSC rate (`knc.core_khz=` override), so `calibrate_APIC_clock` measures the LAPIC timer against the TSC; wall clock from the ring header; PGE cleared, ARAT and TSC flags forced; halt with a POST code on restart, power-off and panic; decompressor writes `K0`/`K1` |
| 0013 | `x86: inline assembly without CMOV` | `Kconfig.cpu`, `asm/uaccess_64.h` | `mask_user_address` clamps with compare, branch, move instead of `cmova`; `X86_CMOV` off so `ffs/fls` take their branch variants (measured: `cmova` on every user-access path) |
| 0014 | `lib/crypto: no x86 SIMD implementations` | `lib/crypto/Kconfig` | The SHA/AES/GHASH/BLAKE2s/ChaCha/Poly1305/Curve25519 x86 assembly is default-on for X86_64 with no switch; excluded under X86_KNC (4000+ SIMD instructions that would only ever be dispatched around) |
| 0015 | `x86/lib: cached copy instead of non-temporal stores` | `arch/x86/lib/copy_user_uncached_64.S` | `__copy_user_nocache` assembled with `mov` and no `sfence` (both SSE2) |
| 0016 | `x86/knc: tty on the host/card ring` | `kernel/knc_tty.c`, `kernel/knc_earlycon.c` | `ttyPHI0`: output through the shared console ring under a lock, input polled from the host-to-card ring every 10 ms; `console=ttyPHI0` gives init its `/dev/console` |

Not in the series, deliberately: SMP bring-up. Intel's card kernel used
the standard INIT/SIPI sequence (`arch/x86/kernel/smpboot.c` in the k1om
tree only parallelizes it), so mainline's is expected to work; the first
boot decides.

## Build

```sh
card/kernel/build.sh all        # fetch, patch, configure, build, audit
card/kernel/build.sh configure  # after editing config/knc.config
card/kernel/build.sh build && card/kernel/build.sh audit
```

`build.md` explains the steps and why `CC=knc-cc`. Output:
`card/kernel/build/out/arch/x86/boot/bzImage` and `vmlinux`.

## What the host sees while it boots

POST codes (`phictl postcode`, decoded by `phi-regs`): `K0` decompressor
entered, `K1` decompressed, `K2` platform layer, `K3` SFI parsed, `K4`
timer init, `K5` CPUs and I/O APIC registered, `K6` ring console
attached, `K7` late initcalls done, `KH` halted, `KP` panic, `KE` no SFI
tables. Console lines arrive in the ring (`phictl console`) from the
moment `earlyprintk=phiring` is parsed, which is before `K2`.

## Boot path recap

32-bit entry (`startup_32` in `arch/x86/boot/compressed/head_64.S`) with
`%esi` = the bootstrap's `boot_params`. The decompressor loads its own GDT
immediately, so the bootstrap's descriptor order (SSDG 2.2.4.2) does not
matter. `verify_cpu` passes once XMM/XMM2 are not required (patch 0002).
Addresses above 4 GiB (the DBOX POST register) are identity-mapped on
demand by the decompressor's page fault handler.

## Sites inspected in v7.2.3 before writing the series (2026-09-13)

| Patch | Site | Finding |
| --- | --- | --- |
| 0001, 0012 | `arch/x86/kernel/cpu/hypervisor.c` `hypervisors[]`, `arch/x86/kernel/jailhouse.c` | The hook is the hypervisor table: `x86_hyper_jailhouse` provides `.detect` and `.init.init_platform`, called from `init_hypervisor_platform()` in `setup_arch` after `e820__memory_setup` and `parse_early_param`, before `tsc_early_init`, `init_mem_mapping` and the APIC code. `x86_hyper_knc` does the same with the native type. |
| 0002 | `arch/x86/Kconfig.cpufeatures` lines 48 to 99 | `CMOV` (via `X86_CMOV`), `PSE`, `PGE`, `FXSR`, `XMM`, `XMM2` are required on `X86_64`. `verify_cpu.S` builds its `SSE_MASK` from `REQUIRED_MASK0`, so once `XMM`/`XMM2` are not required the SSE test passes without touching that assembly. `FXSR` stays required (KNC has it). |
| 0002 | `arch/x86/kernel/head_64.S:232` | `btsl $X86_CR4_PGE_BIT, %ecx` unconditionally for the boot CPU and every AP. `arch/x86/mm/init.c:243` sets PGE only behind the feature bit, which patch 0012 clears. |
| 0002 | `arch/x86/kernel/verify_cpu.S` | Intel family > 6 takes `.Lverify_cpu_clear_xd`, which reads and may write `IA32_MISC_ENABLE`. Called by the decompressor, `head_64.S` and the AP trampoline. |
| 0004 | `arch/x86/include/asm/vdso/processor.h:13`, `asm/processor.h:621` | `native_pause()` is a bare `pause`; `BASE_PREFETCH` is `prefetcht0` on 64-bit. |
| 0006 | `arch/x86/include/asm/nops.h`, `asm/bitops.h:245` | 64-bit NOPs 3 to 11 are `0F 1F` forms; `__ffs` is `tzcnt`. Intel's k1om tree used `66 90` NOPs and `bsf`. |
| 0007 | `arch/x86/include/asm/special_insns.h:185-203` | `clflush()`, `clflushopt()`, `clwb()` are the only inline sites; `mm/pat/set_memory.c` prefers `wbinvd` when CLFLUSH is absent. |
| 0008 | `arch/x86/kernel/cpu/intel.c`, `cpu/microcode/core.c` | `early_init_intel` reads the microcode revision and platform id for every family >= 6, `intel_unlock_cpuid_leafs` and the FAST_STRING check touch `IA32_MISC_ENABLE` for `x86_vfm >= DOTHAN`; `load_ucode_bsp` runs before `setup_arch`. Intel's tree: "Neither Aubrey Isle nor Knights Corner implement IA32_MISC_ENABLES". |
| 0009 | `arch/x86/kernel/apic/io_apic.c` `mp_register_ioapic`, `io_apic_init_mappings` | Address is `u32` and lives in `struct mpc_ioapic`; `io_apic_set_fixmap` already takes `phys_addr_t`. |
| 0012 | `arch/x86/kernel/apic/apic.c` `calibrate_APIC_clock` | With `tsc_khz` known it measures the LAPIC timer against the TSC; no PIT or preset period needed. |
| risk | `arch/x86/kernel/cpu/intel.c` `init_intel` | Remaining MSR reads are guarded by CPUID features (DS, TME, split lock) or by `_safe` helpers; the first boot verifies. |
