# What Intel's card kernel did (reference for the forward-port)

Source tree: `linux-2.6.38.8+mpss3.5.1` as published in
`github.com/cosmoss-jigu/solros` (`phi-kernel/`). Files read on
2026-09-13: `arch/x86/kernel/intelmic.c` (616 lines),
`arch/x86/platform/sfi/sfi.c` (134 lines), `arch/x86/include/asm/mic_def.h`.
Nothing is copied; this records the hardware contract those files encode.

## Platform hooks (`x86_mic_early_setup`, intelmic.c:479)

```
x86_init.resources.probe_roms       = x86_init_noop
x86_init.resources.reserve_resources = x86_init_noop
x86_init.timers.timer_init          = mic_timer_init_common   (KNC: ETC clocksource if enabled, else noop)
x86_init.irqs.pre_vector_init       = x86_init_noop
x86_platform.calibrate_tsc          = intel_mic_calibrate_tsc
x86_platform.get_wallclock          = returns 0
x86_platform.set_wallclock          = returns -1
machine_ops.shutdown                = _mic_shutdown           (doorbell to the host)
legacy_pic                          = &null_legacy_pic
no_sync_cmos_clock                  = 1
```

The same set as mainline's jailhouse guest, which confirms the template
choice in `docs/decisions/0005`.

## Core frequency (intelmic.c:94-143, KNC branch)

```
SPAD4 (SBOX 0xAB30) fields:
  bits 3:0    thread_mask         enabled threads per core (0xF = 4)
  bits 5:4    cache_size          0,1,2 = 512 KiB, 3 = 256 KiB
  bits 9:6    gbox_channel_count  0-based memory channel count
  bits 29:25  icc_divider         reference clock = 4000 / icc_divider MHz
  bit  30     soft_reset          1 if this boot came from a soft reset
  bit  31     internal_flash

CURRENT_CLK_RATIO (SBOX 0x3004) fields:
  bits 8:1    fb                  feedback divider
  bits 10:9   ff                  feedforward divider code: 3 -> 1, 2 -> 2, else 4

core MHz = (4000 / icc_divider) * fb / div(ff)
```

Measured on this card at first contact: `SPAD4 = 0x2800e6cf`, which
decodes to thread_mask 0xF, cache 512 KiB, 12 channels (11 + 1), ICC
divider 20 (reference 200 MHz), no soft reset. For 1100 MHz the ratio
register must hold fb = 11 with div 2 (ff = 2), or fb = 22 with div 4;
`phictl info` now reads and decodes it.

`intel_mic_calibrate_tsc` returns this frequency in kHz (with a small
TSC-compensation term), so the TSC runs at core frequency and the LAPIC
timer calibration uses the same value.

## CPUs and I/O APIC (arch/x86/platform/sfi/sfi.c)

```
register_lapic_address(sfi_lapic_addr)          from the SFI system table
sfi_table_parse(SFI_SIG_CPUS,  sfi_parse_cpus)   each entry: apic_id -> mp_sfi_register_lapic
sfi_table_parse(SFI_SIG_APIC,  sfi_parse_ioapic) each entry: phys_addr (64-bit) -> mp_register_ioapic(i, phys_addr, gsi_top)
```

That is the whole consumer: two tables, APIC IDs and I/O APIC base
addresses. Memory comes from the e820 map the bootstrap places in
`boot_params` (POST code 0x05 "Program E820 table"); `intelmic.c` never
touches e820.

## I/O APIC pin map (intelmic.c:529-590, `mic_construct_default_ioirq_mptable`)

| Pin | Source | Registered |
| --- | --- | --- |
| 0-3 | DMA completion channels 0-3 | not used |
| 4-7 | DMA completion channels 4-7 | yes, edge |
| 8-11 | display, unused | no |
| 12 | PMU | yes, edge |
| 13 | Thermal | yes, edge |
| 14 | SPI | no |
| 15 | SBOX global APIC error | yes, level (flag 13) |
| 16 | Machine check (MCA) | yes, edge |
| 17-24 | Remote DMA completion channels 0-7 | yes, edge |
| 25 | PSMI | no |

All entries use `srcbus 0`, `dstapic MP_APIC_ALL`, `mp_INT`. The KNC
platform layer registers the same pins through the modern
`mp_register_ioapic` + `mp_irqs` path, or simply lets the doorbell/DMA
drivers request GSIs by number.

## Shutdown and doorbells

`_mic_shutdown` writes a doorbell (`SBOX_SDBIC1`, KNC value 0xCC94) with a
state code so the host knows why the card stopped; the host sees it as an
interrupt. `mic_shutdown_isr` handles the reverse direction on
`sbox_irqs[1]`. The forward-port keeps this contract only in spirit: the
card writes its state into the ring region header and `phid` polls it.

## SMPT (ML1OM only)

`mic_smpt_init` programs a 1:1 mapping of host 0..512 GiB into the card
system range; on KNC the host driver programs SMPT instead. Matches
`docs/research/memory-map.md`.
