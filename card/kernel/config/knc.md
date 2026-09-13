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
