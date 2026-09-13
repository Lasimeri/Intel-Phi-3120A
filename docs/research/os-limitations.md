# What a stock OS must change to run on Knights Corner

Source: Intel Xeon Phi Coprocessor System Software Developers Guide (SSDG),
document 328207-002, section 4.2 "Limitations for Shrink-Wrapped Operating
Systems", plus sections 2.2.3 to 2.2.5 on boot. Text extracted with
`pdftotext`. Right column: what this project does about it, with the mainline
precedent used.

| SSDG | Statement | Kernel-port consequence |
| --- | --- | --- |
| 4.2.1 | The x86-64 ABI uses XMM registers, which do not exist | Kernel builds with `-mno-sse` anyway. Userland ABI is the subject of decisions/0002. |
| 4.2.2 | No PCH: no legacy PC devices. A serial console exists on the SBOX I2C bus. Ethernet can be emulated over shared memory | No 8250, no i8042, no PIC, no PIT. Console and network are the ring transport in `docs/spec/ring-protocol.md`. |
| 4.2.3 | Long mode is supported; **the compatibility submode is not** | No 32-bit userland. `CONFIG_IA32_EMULATION=n`. |
| 4.2.4 | Local APIC per hardware thread with expanded ID fields; SBOX contains a LAPIC with 8 ICRs for host-to-card and card-to-card interrupts | Standard xAPIC code path should work for the per-thread LAPICs. The SBOX ICRs are what the host writes to boot and to signal the card. |
| 4.2.5 | I/O APIC at a fixed 64-bit base address; its pins are SBOX interrupt sources (DMA, thermal), not ISA/PCI | Register the I/O APIC from platform code with a 64-bit address; do not parse MP/ACPI tables. Precedent: `arch/x86/kernel/jailhouse.c` registers the I/O APIC via `mp_register_ioapic`. |
| 4.2.6 | No PIT, RTC, ACPI timer, HPET. LAPIC timer is the only timer. Calibrate it from an SBOX MMIO register that reports the core frequency. Time of day must be obtained from the host | `x86_init.timers.timer_init` and `x86_platform.calibrate_tsc/cpu` overridden. `get_wallclock` reads a value the host writes into the shared ring header. Precedent: jailhouse `jailhouse_get_tsc`, `jailhouse_get_wallclock`. |
| 4.2.7 | No Debug Store (BTS, PEBS) | perf uses the KNC PMU driver already in mainline (`arch/x86/events/intel/knc.c`); no DS. |
| 4.2.8 | Thermal throttling via the TMU and SBOX interrupts, not ACPI; P-states set through SBOX registers | Out of scope for SSH. Cores run at the frequency the bootstrap sets. |
| 4.2.9 | No Pending Break Enable | Nothing needed. |
| 4.2.10 | **No global pages.** CPUID reports PGE, but writing CR4.PGE faults with #GP | Force-clear `X86_FEATURE_PGE` early (`setup_clear_cpu_cap`) so `cr4_init` never sets it; `Kconfig.cpufeatures` currently *requires* PGE on 64-bit, so that requirement is relaxed for the KNC config. |
| 4.2.11 | No CNXT-ID | Nothing needed. |
| 4.2.12 | No PREFETCH; VPREFETCH instead | `asm/processor.h` `prefetch()` and `prefetchw()` become no-ops under the KNC config; compiler flag `-mno-prefetch`-equivalent via feature removal; audit verifies. |
| 4.2.13 | 40-bit physical addresses; 32 bits in 32-bit mode | Irrelevant to a 64-bit kernel except during the 32-bit entry stub, which runs identity-mapped below 4 GiB. |
| 4.2.14 | No PSN | Nothing needed. |
| 4.2.15 | No Pentium Pro MCA; Pentium-style MCE only | `CONFIG_X86_MCE=n` for the KNC config. |
| 4.2.16 | No VMX | `CONFIG_KVM=n`. |
| 4.2.17 | CPUID max basic leaf 4, extended 0x80000008, plus 0x20000001 | Code that probes leaves 5, 6, 7, 0xB, 0xD must tolerate their absence (mainline already guards on max leaf). Topology enumeration cannot use leaf 0xB; APIC IDs come from the SFI CPU table instead. |
| 4.2.17.1 | LAPIC timer keeps running in C3, but leaf 6 does not exist to advertise it | Set `X86_FEATURE_ARAT` from platform code. |
| 4.2.18 | Unsupported instructions (see isa-deletions.md) | Compiler features, kernel asm audit. |

## Boot facts that constrain the kernel (SSDG 2.2.3, 2.2.4)

- The bootstrap (`fboot1`, in flash) initializes cores, GDDR, uncore, boots
  the APs into 64-bit mode and parks them, then waits for the host to download
  an OS image (POST code 0x12).
- On download it "creates the boot parameter structure", transitions to
  **32-bit protected mode with paging disabled**, and jumps to the kernel's
  **32-bit entry point** with `%esi` pointing at `struct boot_params`. The
  16-bit and 64-bit entry points are not supported.
- The GDT the bootstrap leaves behind does not match the boot protocol's
  selector layout, so the bootstrap sets the `KEEP_SEGMENTS` load flag. The
  decompressor must not reload segments before setting up its own GDT (it
  does this correctly in mainline; `KEEP_SEGMENTS` was removed from the
  protocol in 5.15, which is a patch point: the 32-bit entry must tolerate
  the bootstrap's GDT).
- CPU count, memory map, and other hardware configuration are reported via
  **SFI tables** (Simple Firmware Interface 0.7). Mainline removed SFI in
  Linux 5.12. The KNC platform layer brings back a minimal parser for the
  `SYST`, `CPUS`, `MMAP`, and `APIC` tables. The SFI spec places the system
  table signature in the 0xE0000 to 0xFFFFF physical range.
- APs are already in 64-bit mode and parked by the bootstrap; the Intel kernel
  used a modified SMP boot (`CONFIG_PARALLEL_AP_BOOT` in `k1om.uconfig`).
  Whether the standard INIT/SIPI sequence wakes them is an open question for
  phase P2; the Intel tree's `arch/x86/kernel/intelmic.c` and `smpboot.c` diff
  answer it.

## Intel's own configuration (for calibration)

From `arch/x86/configs/k1om.uconfig` in the `linux-2.6.38.8+mpss3.5.1` tree:
`CONFIG_MK1OM`, `CONFIG_X86_MICPCI`, `CONFIG_X86_EXTENDED_PLATFORM`,
`CONFIG_SFI=y`, `CONFIG_NR_CPUS=255`, `CONFIG_SCHED_SMT/MC`,
`CONFIG_MIC_PM`, `CONFIG_MIC_CPUIDLE`, `CONFIG_MIC_CPUFREQ`,
`CONFIG_PARALLEL_AP_BOOT`, `CONFIG_TRANSPARENT_HUGEPAGE_ALWAYS`.
The platform file `arch/x86/kernel/intelmic.c` defines the SBOX base as card
physical `0x08007D0000` and the SMPT entry format (host address bits 31:2,
no-snoop bit 0).
