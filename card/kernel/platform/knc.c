// SPDX-License-Identifier: GPL-2.0-only
/*
 * knc.c: Knights Corner (Xeon Phi x100) platform layer for a mainline kernel.
 *
 * STATUS: DRAFT, NOT YET COMPILED. Written against the shape of
 * arch/x86/kernel/jailhouse.c on current mainline; phase P3 turns it into
 * patch 6 of the series in card/kernel/README.md.
 *
 * What the hardware lacks and what this file does about it is tabulated in
 * docs/research/os-limitations.md (SSDG 328207-002 section 4.2). Register
 * offsets are from docs/spec/sbox-registers.md.
 */

#include <linux/init.h>
#include <linux/io.h>
#include <linux/kernel.h>
#include <linux/memblock.h>
#include <linux/types.h>

#include <asm/apic.h>
#include <asm/cpufeature.h>
#include <asm/io_apic.h>
#include <asm/setup.h>
#include <asm/x86_init.h>

/* SBOX register block, card physical (SSDG 2.1.12, intelmic.c MIC_SBOX_BASE). */
#define KNC_SBOX_PHYS		0x08007D0000ULL
#define KNC_SBOX_SIZE		0x10000
/* DBOX block; the POST code register lives at +0x242c (mic_x100.h). */
#define KNC_DBOX_PHYS		0x08007C0000ULL
#define KNC_POSTCODE		0x242c
/*
 * Core frequency register: SSDG 4.2.6 says an SBOX MMIO register reports
 * the current CPU frequency for LAPIC timer calibration. The exact offset is
 * taken from micsboxdefine.h (vendor/) in phase P3; placeholder name here.
 */
#define KNC_SBOX_COREFREQ	0x0 /* TODO(P3): from micsboxdefine.h */

static void __iomem *knc_sbox;
static void __iomem *knc_dbox;

/* Card-side progress codes, visible to the host via `phictl postcode`. */
#define KNC_POST_PLATFORM_INIT	0x80
#define KNC_POST_SFI_PARSED	0x81
#define KNC_POST_TIMER_INIT	0x82
#define KNC_POST_SMP_PREPARE	0x83

/**
 * knc_postcode() - Write a progress code the host can read.
 * @code: value for the low byte of the DBOX POST code register.
 */
void knc_postcode(u8 code)
{
	if (knc_dbox)
		writel(code, knc_dbox + KNC_POSTCODE);
}

/* Filled by knc_sfi.c from the bootstrap's SFI tables (SSDG 2.2.4.2). */
struct knc_sfi_info {
	unsigned int num_cpus;
	u32 apic_ids[256];
	u64 ioapic_phys;	/* I/O APIC base is 64-bit on KNC (SSDG 4.2.5) */
	u32 lapic_khz;		/* 0 if not reported */
};
extern int knc_sfi_parse(struct knc_sfi_info *info);

static struct knc_sfi_info knc_sfi;

static void __init knc_parse_smp_config(void)
{
	unsigned int i;

	for (i = 0; i < knc_sfi.num_cpus; i++)
		topology_register_apic(knc_sfi.apic_ids[i], CPU_ACPIID_INVALID, true);

	/*
	 * SSDG 4.2.5: I/O APIC at a fixed 64-bit address; its pins are SBOX
	 * interrupt sources, not ISA IRQs. Register it without an ISA identity
	 * mapping.
	 */
	if (knc_sfi.ioapic_phys) {
		struct ioapic_domain_cfg cfg = {
			.type = IOAPIC_DOMAIN_STRICT,
			.ops = &mp_ioapic_irqdomain_ops,
		};
		mp_register_ioapic(0, knc_sfi.ioapic_phys, gsi_top, &cfg);
	}
}

/*
 * SSDG 4.2.6: no PIT, RTC, ACPI timer or HPET. The LAPIC timer is the only
 * timer and must be calibrated from the SBOX core-frequency register.
 * TSC runs at core frequency on KNC (Intel's kernel used it as the
 * clocksource), so the same value serves calibrate_tsc.
 */
static unsigned long knc_get_tsc(void)
{
	u32 khz = knc_sfi.lapic_khz;

	if (!khz && knc_sbox)
		khz = readl(knc_sbox + KNC_SBOX_COREFREQ); /* TODO(P3): decode */
	return khz;
}

static void __init knc_timer_init(void)
{
	knc_postcode(KNC_POST_TIMER_INIT);
	/* Nothing to do: no PIT to program; LAPIC timer setup is generic. */
}

/* Wall clock: the host writes its epoch into the ring region header. */
static void knc_get_wallclock(struct timespec64 *now)
{
	extern u64 knc_ring_host_epoch_ns(void); /* knc_earlycon.c */
	u64 ns = knc_ring_host_epoch_ns();

	*now = ns64_to_timespec64(ns);
}

static int knc_set_wallclock(const struct timespec64 *now)
{
	return -ENODEV;
}

static void __init knc_init_platform(void)
{
	knc_sbox = early_ioremap(KNC_SBOX_PHYS, KNC_SBOX_SIZE);
	knc_dbox = early_ioremap(KNC_DBOX_PHYS, KNC_SBOX_SIZE);
	knc_postcode(KNC_POST_PLATFORM_INIT);

	if (knc_sfi_parse(&knc_sfi))
		panic("KNC: no usable SFI tables from the bootstrap");
	knc_postcode(KNC_POST_SFI_PARSED);

	/* SSDG 4.2.2 and 4.2.6: none of the PC-AT devices exist. */
	x86_init.irqs.pre_vector_init		= x86_init_noop;
	x86_init.timers.timer_init		= knc_timer_init;
	x86_init.mpparse.find_mptable		= x86_init_noop;
	x86_init.mpparse.early_parse_smp_cfg	= x86_init_noop;
	x86_init.mpparse.parse_smp_cfg		= knc_parse_smp_config;
	x86_init.pci.arch_init			= x86_init_noop;

	x86_platform.calibrate_cpu		= knc_get_tsc;
	x86_platform.calibrate_tsc		= knc_get_tsc;
	x86_platform.get_wallclock		= knc_get_wallclock;
	x86_platform.set_wallclock		= knc_set_wallclock;
	x86_platform.legacy.rtc			= 0;
	x86_platform.legacy.warm_reset		= 0;
	x86_platform.legacy.i8042		= X86_LEGACY_I8042_PLATFORM_ABSENT;
	legacy_pic				= &null_legacy_pic;

	/*
	 * SSDG 4.2.10: CPUID advertises PGE but CR4.PGE faults. Clear it before
	 * cr4_init() runs on any CPU. SSDG 4.2.17.1: the LAPIC timer keeps
	 * running in C3 but leaf 6 does not exist to say so.
	 */
	setup_clear_cpu_cap(X86_FEATURE_PGE);
	setup_force_cpu_cap(X86_FEATURE_ARAT);
	/* CPUID says FXSR=1, CMOV=0, CLFSH=0; nothing to force there. */
}

/*
 * Detection: family 0x0B model 0x01 (ISA reference CPUID appendix). Hooked
 * from x86_init.oem.arch_setup or a platform subarch check in setup_arch();
 * the exact hook is chosen when patch 6 is written.
 */
static u32 __init knc_detect(void)
{
	u32 eax, ebx, ecx, edx;

	cpuid(1, &eax, &ebx, &ecx, &edx);
	if (((eax >> 8) & 0xf) == 0x0b && ((eax >> 4) & 0xf) == 0x01)
		return 1;
	return 0;
}

void __init knc_platform_setup(void)
{
	if (!IS_ENABLED(CONFIG_X86_KNC) || !knc_detect())
		return;
	knc_init_platform();
}
