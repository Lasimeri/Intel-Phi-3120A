# card/kernel/platform/knc.c (draft)

The KNC platform layer, drafted before the kernel tree is pinned so that
the design is reviewable now. It is **not compiled yet**; phase P3 turns it
into patch 6 of the series and adjusts it to the exact hook names of the
pinned tag.

## Design

Modeled line by line on `arch/x86/kernel/jailhouse.c` in mainline, which is
the existing precedent for "x86 with no legacy devices whose CPU list and
timing come from a boot-time table". The mapping from SSDG 4.2 to code:

| SSDG | Code |
| --- | --- |
| 4.2.2, 4.2.6 (no PIC/PIT/RTC/HPET/i8042) | `x86_init.irqs.pre_vector_init = x86_init_noop`, `timer_init` override, `legacy.rtc/warm_reset/i8042` cleared, `legacy_pic = &null_legacy_pic` |
| 4.2.4, 2.2.4.2 (CPU list from SFI) | `parse_smp_cfg` registers APIC IDs from the SFI `CPUS` table via `topology_register_apic` |
| 4.2.5 (I/O APIC at a 64-bit base, pins are SBOX sources) | `mp_register_ioapic` with the 64-bit address and a strict domain, no ISA overrides |
| 4.2.6 (LAPIC timer calibration) | `calibrate_tsc`/`calibrate_cpu` return the core frequency from SFI or the SBOX register |
| 4.2.6 (time of day from the host) | `get_wallclock` reads `host_epoch_ns` from the ring region header |
| 4.2.10 (CR4.PGE faults) | `setup_clear_cpu_cap(X86_FEATURE_PGE)` |
| 4.2.17.1 (ARAT without leaf 6) | `setup_force_cpu_cap(X86_FEATURE_ARAT)` |
| POST codes | `knc_postcode()` writes DBOX+0x242c at milestones so the host sees progress before the console works |

## Open items (marked `TODO(P3)` in the source)

- The SBOX core-frequency register offset and encoding, from
  `micsboxdefine.h` in `vendor/mpss-3.8.6`.
- The hook used to call `knc_platform_setup()` early enough (before
  `x86_init` consumers run). Candidates: `x86_init.oem.arch_setup`, or a
  check on `boot_params.hdr.hardware_subarch` if the bootstrap sets one.
- Whether `PGE` must also be dropped from `Kconfig.cpufeatures` (it is
  currently `X86_REQUIRED_FEATURE_PGE` on 64-bit, so yes, patch 2).

## Companion files

`knc_sfi.c` (SFI parser, patch 7) and `knc_earlycon.c` (ring console,
patch 8) are drafted alongside in phase P3; their interfaces are declared
here as `extern` so the dependency direction is visible.
