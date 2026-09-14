# card/kernel/config/knc.config

Kconfig fragment merged onto `x86_64_defconfig`. Each block, and why:

| Block | Reason |
| --- | --- |
| `X86_KNC`, SMP, 256 CPUs, SMT/MC scheduling | 57 cores x 4 threads = 228 hardware threads. Intel used `NR_CPUS=255` in `k1om.uconfig`. |
| No ACPI, PCI, HPET, MP tables, RTC, 8250, VT, input | SSDG 4.2.2 and 4.2.6: none of these devices exist. `PCI=n` because the card kernel does not see a PCI bus; its "devices" are SBOX registers reached through the platform layer. |
| `X86_X2APIC=n` | KNC has xAPIC with expanded ID fields (SSDG 4.2.4); x2APIC MSRs do not exist. |
| `IA32_EMULATION=n`, `X86_16BIT=n`, `X86_VSYSCALL_EMULATION=n` | No compatibility submode (SSDG 4.2.3). |
| `X86_5LEVEL=n` | 40-bit physical addresses; no LA57. |
| `X86_MCE=n` | Pentium-style MCE only (SSDG 4.2.15). |
| `KVM=n` | No VMX (SSDG 4.2.16). |
| `X86_KERNEL_IBT=n`, shadow stack, UMIP, SGX, FRED off | Features that emit `endbr64` or use MSRs the core lacks. |
| All `MITIGATION_*` off | Retpolines insert `lfence`; return thunks and IBRS use MSRs and instructions the core lacks. The card is not a multi-tenant machine. |
| `RANDOMIZE_BASE=n` | KASLR needs RDRAND/RDTSC entropy paths that assume newer CPUs; simpler to debug without. |
| Crypto SIMD variants off | They contain SSE/AVX code selected at runtime; the audit would flag them and they would never run. |
| `HIGH_RES_TIMERS`, `NO_HZ_IDLE` | LAPIC timer is the only timer (SSDG 4.2.6); high-resolution mode makes the scheduler tick unnecessary on idle threads, which matters with 228 of them. |
| initramfs decompressors, devtmpfs, tmpfs | The whole root is a ramdisk. |
| NET/INET/UNIX/PACKET, `ETHERNET=n` | `phinet` provides the netdev; no real Ethernet drivers. |
| perf, MSR, CPUID, printk time, sysrq, modules | Debugging. `perf` uses the mainline KNC PMU driver. |

Options the patch series introduces (`CONFIG_X86_KNC`) will not exist on
an unpatched tree; `merge_config.sh` warns and drops them, which is a
useful signal that the patches are missing.

## Lines added with the patch series (2026-09-13)

| Option | Reason |
| --- | --- |
| `EXPERT=y`, `PROCESSOR_SELECT=y` | Needed to switch off `VT`, `INPUT`, `X86_16BIT`, `X86_VSYSCALL_EMULATION`, `X86_UMIP`, `PCSPKR_PLATFORM` and the non-Intel `CPU_SUP_*` vendors, which are only visible under `EXPERT`. |
| `HYPERVISOR_GUEST=y` | `X86_KNC` depends on it: the platform layer hooks the hypervisor detection table (patch 0001). |
| `CPU_MITIGATIONS=n` plus the individual `MITIGATION_*` | One switch for every speculation mitigation; the individual lines stay for older trees. |
| `PARAVIRT=n`, `KVM_GUEST=n`, `XEN=n`, `JAILHOUSE_GUEST=n`, `INTEL_TDX_GUEST=n`, `AMD_MEM_ENCRYPT=n` | Guest and memory-encryption code paths that would probe CPUID leaves and MSRs the core lacks, or add indirections for nothing. |
| `CPU_SUP_*=n` except Intel, `X86_CPU_RESCTRL=n` | Fewer vendor quirks compiled in; resctrl reads MSRs behind CPUID leaf 7, absent here. |
| `X86_IOPL_IOPERM=n` | No port I/O (patch 0005). |
| `EFI=n`, `KEXEC=n`, `CRASH_DUMP=n`, `SUSPEND=n`, `HIBERNATION=n` | Not applicable to a card booted by the host through the bootstrap. |
| `EARLY_PRINTK=y`, `EARLY_PRINTK_DBGP=n`, `EARLY_PRINTK_USB_XDBC=n` | The ring console registers through `earlyprintk=phiring` (patch 0011); the USB variants would only add port and PCI probes. |

Lines that could not stay: `HPET_TIMER` and `X86_MPPARSE` are `def_bool y`
on x86-64 (harmless: no HPET is found and the platform layer replaces the
MP-table hooks), and `MICROCODE` is `def_bool y` since 6.6, which is why
patch 0008 disables the loader at run time for family 0xb.
| `X86_INTEL_MEMORY_PROTECTION_KEYS=n` | `rdpkru`/`wrpkru` on every context switch path; the feature is CPUID-gated but the instructions would sit in the text. |
| `CRC_OPTIMIZATIONS=n`, `RD_ZSTD=n` | The x86 CRC implementations are PCLMUL/AVX code selected at run time; zstd's BMI2 decoder variants use `lzcnt`. Neither is needed (initramfs is gzip or xz). The lib/crypto SIMD variants have no switch and are excluded by patch 0014. |
| `DRM=n` | The card has no display device; the DRM core carries `movntdqa` copies selected at run time. |
