//! SBOX and DBOX register offsets, bit layouts and decoders.
//!
//! The card exposes two BARs. BAR4 ("MEMBAR1", 128 KiB) holds the DBOX
//! registers in its first 64 KiB and the SBOX registers in its second 64 KiB
//! (SSDG 2.1.12). Every constant in this module documented as an
//! "SBOX offset" is relative to [`SBOX_BASE`]; add the two to get a BAR4
//! offset. The one DBOX-side register this project reads, [`POSTCODE`], is
//! a raw BAR4 offset.
//!
//! Sources, abbreviated as in the crate root. `mic_x100.h` (mainline v5.9)
//! covers everything Intel's mainline host driver touched. The rest comes
//! from `micsboxdefine.h`, which exists in two variants: the MPSS 3.8.6
//! host copy (`include/mic/micsboxdefine.h`, KNF and KNC merged, with
//! KNC-only values under `CONFIG_MK1OM` or carrying a `_K1OM` suffix) and
//! the card kernel copy in Intel's k1om tree
//! (`arch/x86/include/asm/mic/mic_knc/micsboxdefine.h`, KNC only). Where
//! the two disagree the KNC copy wins and the live register was read on
//! this card (see [`CURRENT_CLK_RATIO`]). Bit layouts of the clock and
//! voltage registers are from `mic_knc/micsboxstruct.h` in the same tree;
//! the sensor decodes from MPSS `ras/micras_knc.c` (`mr_get_temp`,
//! `mr_get_volt`, `ratio2freq`); the DMA layouts from MPSS
//! `dma/mic_dma_md.c` and `include/mic/mic_dma_md.h`.

// ---------------------------------------------------------------------------
// BARs

/// Offset of the SBOX register block inside MMIO BAR4 (`MIC_X100_SBOX_BASE_ADDRESS`).
pub const SBOX_BASE: u32 = 0x0001_0000;

/// Size of MMIO BAR4 on this card, measured (`docs/hardware.md`).
pub const MMIO_BAR_SIZE: u64 = 128 * 1024;

/// BAR index of the aperture (card memory) in PCI config space (`MIC_X100_APER_BAR`).
pub const APER_BAR_INDEX: u32 = 0;
/// BAR index of the MMIO register block (`MIC_X100_MMIO_BAR`).
pub const MMIO_BAR_INDEX: u32 = 4;

/// POST code register. **BAR4 offset, not SBOX-relative**: mainline reads it
/// with `mic_mmio_read(&mdev->mmio, MIC_X100_POSTCODE)` without adding the
/// SBOX base (`mic_x100_get_postcode`). Decoded by [`crate::postcode`].
pub const POSTCODE: u32 = 0x242c;

// ---------------------------------------------------------------------------
// Scratchpads

/// First scratchpad register (`MIC_X100_SBOX_SPAD0`, `SBOX_SCRATCH0`). SBOX offset.
pub const SPAD0: u32 = 0xAB20;
/// Number of scratchpad registers the driver model assumes (SPAD0..SPAD15).
pub const SPAD_COUNT: u32 = 16;

/// Scratchpad index carrying download info (`MIC_X100_DOWNLOAD_INFO`), decoded by [`DownloadInfo`].
pub const SPAD_DOWNLOAD_INFO: u32 = 2;
/// Scratchpad index holding the bootstrap's platform word (`MIC_SBOX_SCRATCH4`
/// in `intelmic.c`, `SBOX_SCRATCH4` in `micras_knc.c`), decoded by [`PlatformInfo`].
pub const SPAD_PLATFORM_INFO: u32 = 4;
/// Scratchpad index that receives the firmware image size (`MIC_X100_FW_SIZE`).
pub const SPAD_FW_SIZE: u32 = 5;

/// SBOX offset of scratchpad `n` (panics if `n >= SPAD_COUNT`).
pub const fn spad(n: u32) -> u32 {
    assert!(n < SPAD_COUNT);
    SPAD0 + 4 * n
}

/// Decoded view of scratchpad 2, the bootstrap's download-info word.
///
/// Layout from `mic_x100.h`:
/// - bit 0: `MIC_X100_SPAD2_DOWNLOAD_STATUS`, 1 when the bootstrap is ready
///   to receive an image ("firmware ready")
/// - bits 9:1: `MIC_X100_SPAD2_APIC_ID`, APIC ID of the bootstrap processor
/// - bits 31:12: `MIC_X100_SPAD2_DOWNLOAD_ADDR`, card physical address at
///   which the host must place the kernel image (page aligned)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadInfo(pub u32);

impl DownloadInfo {
    /// True when the bootstrap reports it is waiting for an image.
    pub const fn ready(self) -> bool {
        self.0 & 1 == 1
    }
    /// APIC ID of the bootstrap processor, the target of the boot interrupt.
    pub const fn apic_id(self) -> u32 {
        (self.0 >> 1) & 0x1ff
    }
    /// Card physical address at which to place the kernel image.
    pub const fn download_addr(self) -> u32 {
        self.0 & 0xffff_f000
    }
}

/// Decoded scratchpad 4, the bootstrap's platform description word
/// (`union sboxScratch4RegDef` in Intel's `intelmic.c`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlatformInfo(pub u32);

impl PlatformInfo {
    /// Mask of enabled hardware threads per core (bits 3:0); 0xF means four.
    pub const fn thread_mask(self) -> u32 {
        self.0 & 0xF
    }
    /// L2 size per core in KiB (bits 5:4: 0, 1, 2 mean 512, 3 means 256).
    pub const fn l2_kib(self) -> u32 {
        if (self.0 >> 4) & 0x3 == 3 {
            256
        } else {
            512
        }
    }
    /// Number of memory channels (bits 9:6 hold the count minus one).
    pub const fn memory_channels(self) -> u32 {
        ((self.0 >> 6) & 0xF) + 1
    }
    /// Fused ICC divider (bits 29:25), raw; the reference clock is
    /// [`CORE_VCO_MHZ`] divided by it. 0 means "not fused, nominal".
    pub const fn icc_divider(self) -> u32 {
        (self.0 >> 25) & 0x1F
    }
    /// Reference clock in MHz. A divider of 0 is treated as the nominal 20
    /// (`ICC_NOM` in `micras_knc.c`, `icc_fwd()`), giving 200 MHz; Intel's
    /// `get_core_freq` in `intelmic.c` divides by the raw field and would
    /// fault on 0.
    pub const fn reference_mhz(self) -> u32 {
        let div = self.icc_divider();
        CORE_VCO_MHZ / if div == 0 { ICC_NOMINAL_DIVIDER } else { div }
    }
    /// True if this boot followed a soft reset (bit 30).
    pub const fn soft_reset(self) -> bool {
        (self.0 >> 30) & 1 == 1
    }
    /// True if the flash is an internal (Intel-internal) build (bit 31).
    pub const fn internal_flash(self) -> bool {
        (self.0 >> 31) & 1 == 1
    }
}

// ---------------------------------------------------------------------------
// Boot interrupt

/// First SBOX local-APIC interrupt command register (`MIC_X100_SBOX_APICICR0`).
/// Eight ICRs of 8 bytes each; low dword = vector and control, high dword =
/// destination APIC ID (SSDG 4.2.4, `mic_x100_send_firmware_intr`).
pub const APICICR0: u32 = 0xA9D0;
/// The ICR Intel used for the boot interrupt (`MIC_X100_SBOX_APICICR7`).
pub const APICICR7: u32 = 0xAA08;
/// Bit in the ICR low dword that triggers delivery ("send_icr bit (13)").
pub const ICR_SEND: u32 = 1 << 13;
/// Interrupt vector the bootstrap waits on to start the downloaded OS
/// (`MIC_X100_BSP_INTERRUPT_VECTOR`).
pub const BSP_INTERRUPT_VECTOR: u32 = 229;

/// SBOX offset of the low dword of interrupt command register `n` (0..8).
pub const fn apicicr(n: u32) -> u32 {
    assert!(n < 8);
    APICICR0 + 8 * n
}

// ---------------------------------------------------------------------------
// Reset

/// Reset control register (`MIC_X100_SBOX_RGCR`). Setting bit 0 resets the
/// card back to its bootstrap (`mic_x100_hw_reset`).
pub const RGCR: u32 = 0x4010;
/// Bit in [`RGCR`] that triggers the reset.
pub const RGCR_RESET: u32 = 1;

// ---------------------------------------------------------------------------
// System interrupts (card to host)

/// System interrupt cause register 0 (`MIC_X100_SBOX_SICR0`).
pub const SICR0: u32 = 0x9004;
/// System interrupt enable register 0 (`MIC_X100_SBOX_SICE0`).
pub const SICE0: u32 = 0x900C;
/// System interrupt clear register 0 (`MIC_X100_SBOX_SICC0`).
pub const SICC0: u32 = 0x9010;
/// System interrupt auto-clear register 0 (`MIC_X100_SBOX_SIAC0`).
pub const SIAC0: u32 = 0x9014;
/// MSI-X address register 0 (`MIC_X100_SBOX_MXAR0`; `SBOX_MXAR0_K1OM` in
/// MPSS, whose unsuffixed `SBOX_MXAR0 = 0x9040` is the KNF offset).
pub const MXAR0: u32 = 0x9044;
/// MSI-X pending-bit-array control (`MIC_X100_SBOX_MSIXPBACR`;
/// `SBOX_MSIXPBACR_K1OM` in MPSS, the unsuffixed 0x9080 being KNF).
pub const MSIXPBACR: u32 = 0x9084;
/// Doorbell bits (3:0) in SICR0/SICE0/SICC0/SIAC0 (`MIC_X100_SBOX_DBR_BITS`).
pub const SI_DOORBELL_MASK: u32 = 0xF;
/// DMA bits (15:8) in the same registers (`MIC_X100_SBOX_DMA_BITS`).
pub const SI_DMA_MASK: u32 = 0xFF << 8;

/// First remote DMA status register, used as card-to-host doorbell (`MIC_X100_SBOX_RDMASR0`).
pub const RDMASR0: u32 = 0xB180;
/// Number of RDMASR registers (`MIC_X100_NUM_RDMASR_IRQ`).
pub const RDMASR_COUNT: u32 = 8;
/// System doorbell interrupt control 0 (`MIC_X100_SBOX_SDBIC0`; the
/// `CONFIG_MK1OM` value in MPSS, KNF has it at 0x9030).
pub const SDBIC0: u32 = 0xCC90;

/// SBOX offset of RDMASR register `n` (0..8).
pub const fn rdmasr(n: u32) -> u32 {
    assert!(n < RDMASR_COUNT);
    RDMASR0 + 4 * n
}

// ---------------------------------------------------------------------------
// System memory page table (card to host address window)

/// First system memory page table entry (`MIC_X100_SBOX_SMPT00`, `SBOX_SMPT00` in `intelmic.c`).
/// Entry format: [`crate::memory::smpt_entry`].
pub const SMPT00: u32 = 0x3100;
/// Number of SMPT entries (`mic_x100_smpt_hw_init`: `num_reg = 32`).
pub const SMPT_COUNT: u32 = 32;

/// SBOX offset of SMPT entry `n` (0..32).
pub const fn smpt(n: u32) -> u32 {
    assert!(n < SMPT_COUNT);
    SMPT00 + 4 * n
}

// ---------------------------------------------------------------------------
// Core clock and voltage

/// Current core PLL ratio (`SBOX_CURRENTRATIO` in the k1om
/// `mic_knc/micsboxdefine.h`, read by Intel's `mic_cpufreq.c`;
/// `MIC_SBOX_CURRENT_CLK_RATIO` under `CONFIG_MK1OM` in `intelmic.c`). SBOX
/// offset. Layout (`sboxCurrentratioReg` in `micsboxstruct.h`): bits 11:0
/// the ratio the cores run at now, bits 27:16 the goal ratio; both use the
/// [`ClockRatio`] encoding, and differ from [`COREFREQ`] under throttling.
///
/// Not 0x3004: that is the KNF (`CONFIG_ML1OM`) offset, which the merged
/// MPSS `micsboxdefine.h` lists without a guard and which reads 0 on this
/// card. Measured 2026-09-17 on the card (`devmem` through `phictl exec`,
/// recorded in `docs/spec/sbox-registers.md`): SBOX+0x402C = 0x04160416,
/// SBOX+0x3004 = 0x00000000, COREFREQ = 0x80010416.
pub const CURRENT_CLK_RATIO: u32 = 0x402C;
/// Core frequency control register (`SBOX_COREFREQ`, `CONFIG_MK1OM` branch
/// of `micsboxdefine.h`; KNF has it at 0x4040). SBOX offset. Layout
/// (`sboxCorefreqReg`): bits 11:0 the programmed ratio ([`ClockRatio`]),
/// bit 15 fuse ratio, bit 16 async mode, bits 29:26 throttle ratio step,
/// bit 30 throttle jump, bit 31 booted.
pub const COREFREQ: u32 = 0x4100;
/// Core voltage register (`SBOX_COREVOLT`, KNC; KNF 0x4044). SBOX offset.
/// Bits 7:0 are a VR12 SVID code ([`sensors::vcore_mv`]).
pub const COREVOLT: u32 = 0x4104;
/// Elapsed time counter, low half (`SBOX_ELAPSED_TIME_LOW`, `micsboxdefine.h`). SBOX offset.
pub const ELAPSED_TIME_LOW: u32 = 0x1074;
/// High half of the elapsed time counter (`SBOX_ELAPSED_TIME_HIGH`).
pub const ELAPSED_TIME_HIGH: u32 = 0x1078;
/// The PLL reference the ICC divider divides, in MHz (`CORE_VCO` in `intelmic.c`).
pub const CORE_VCO_MHZ: u32 = 4000;
/// Nominal ICC divider assumed when scratchpad 4 reports 0 (`ICC_NOM` in `micras_knc.c`).
pub const ICC_NOMINAL_DIVIDER: u32 = 20;

/// Decoded core PLL ratio word (`union mclkRatioEncoding` in Intel's
/// `intelmic.c`): the low 12 bits of [`CURRENT_CLK_RATIO`] or [`COREFREQ`].
/// Feedback multiplier in bits 8:1, feed-forward divider code in bits 10:9.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockRatio(pub u32);

impl ClockRatio {
    /// The 12-bit ratio code in bits 11:0 (the current ratio in `CURRENTRATIO`).
    pub const fn code(self) -> u32 {
        self.0 & 0xfff
    }
    /// The goal ratio code in bits 27:16 of `CURRENTRATIO` (`goalratio`).
    pub const fn goal_code(self) -> u32 {
        (self.0 >> 16) & 0xfff
    }
    /// Feedback divider (bits 8:1).
    pub const fn feedback(self) -> u32 {
        (self.0 >> 1) & 0xFF
    }
    /// Feedforward divider code (bits 10:9).
    pub const fn feedforward_code(self) -> u32 {
        (self.0 >> 9) & 0x3
    }
    /// Feedforward divider value (`BITS_TO_DIV`: 3 means 1, 2 means 2, else 4).
    pub const fn feedforward_div(self) -> u32 {
        match self.feedforward_code() {
            3 => 1,
            2 => 2,
            _ => 4,
        }
    }
    /// Core frequency in MHz given the platform word (`get_core_freq` in
    /// `intelmic.c`): reference times feedback over feed-forward. No range
    /// check; [`sensors::core_khz`] applies Intel's PLL table and returns
    /// `None` for codes outside it. Both agree on every code inside it.
    pub const fn core_mhz(self, platform: PlatformInfo) -> u32 {
        platform.reference_mhz() * self.feedback() / self.feedforward_div()
    }
}

// ---------------------------------------------------------------------------
// Sensors

/// Thermal status register (`SBOX_THERMAL_STATUS`, `micsboxdefine.h`). SBOX offset.
/// Bit 31 valid, bits 30:22 the TMU die temperature ([`sensors::tmu_temp`]).
pub const THERMAL_STATUS: u32 = 0x1018;
/// Fan controller status 2 (`SBOX_STATUS_FAN2`): VDDG regulator temperature in bits 19:12.
pub const STATUS_FAN2: u32 = 0x1028;
/// Board temperatures 1 (`SBOX_BOARD_TEMP1`): air inlet bits 8:0 (valid bit 15),
/// VCCP regulator bits 24:16 (valid bit 31).
pub const BOARD_TEMP1: u32 = 0x1030;
/// Board temperatures 2 (`SBOX_BOARD_TEMP2`): GDDR bits 8:0 (valid bit 15),
/// GDDR regulator bits 24:16 (valid bit 31).
pub const BOARD_TEMP2: u32 = 0x1034;
/// Board voltage sense (`SBOX_BOARD_VOLTAGE_SENSE`), raw; no decode in any source used.
pub const BOARD_VOLTAGE_SENSE: u32 = 0x1038;
/// Current die temperatures: three 10-bit fields per register, three
/// consecutive registers, nine sensors (`SBOX_CURRENT_DIE_TEMP0..2`).
pub const CURRENT_DIE_TEMP0: u32 = 0x103C;
/// Maximum die temperatures, same layout (`SBOX_MAX_DIE_TEMP0..2`).
pub const MAX_DIE_TEMP0: u32 = 0x1048;

/// Decoding of the sensor registers, shared by the host tool and, in C,
/// by the card's hwmon driver (kernel patch 0027). Formulas follow
/// `mr_get_temp`, `svid2volt` and `mr_mt_cf_r2f` in MPSS `ras/micras_knc.c`.
///
/// The board, VDDG and TMU fields are filled by SMC telemetry broadcasts
/// that only Intel's I2C driver requests; on this port they read 0 and
/// the valid bits stay clear (`docs/results/2026-09-16-sensors.md`).
pub mod sensors {
    /// The nine die temperatures in degrees C from the three registers
    /// (bits 9:0, 19:10, 29:20 of each). 0 means an unfused sensor.
    pub const fn die_temps(regs: [u32; 3]) -> [u16; 9] {
        let mut t = [0u16; 9];
        let mut i = 0;
        while i < 9 {
            t[i] = ((regs[i / 3] >> (10 * (i % 3))) & 0x3ff) as u16;
            i += 1;
        }
        t
    }

    /// (inlet, VCCP regulator) from BOARD_TEMP1, or (GDDR, GDDR regulator)
    /// from BOARD_TEMP2; `None` for a field without its valid bit.
    pub const fn board_temps(reg: u32) -> (Option<u16>, Option<u16>) {
        let low = if reg & (1 << 15) != 0 { Some((reg & 0x1ff) as u16) } else { None };
        let high = if reg & (1 << 31) != 0 {
            Some(((reg >> 16) & 0x1ff) as u16)
        } else {
            None
        };
        (low, high)
    }

    /// VDDG regulator temperature from STATUS_FAN2 (bits 19:12, no valid bit).
    pub const fn vddg_temp(fan2: u32) -> u16 {
        ((fan2 >> 12) & 0xff) as u16
    }

    /// TMU die temperature from THERMAL_STATUS (bits 30:22), if bit 31 says valid.
    pub const fn tmu_temp(status: u32) -> Option<u16> {
        if status & (1 << 31) != 0 {
            Some(((status >> 22) & 0x1ff) as u16)
        } else {
            None
        }
    }

    /// Core voltage in millivolts from the COREVOLT SVID code (VR12:
    /// 250 mV plus 5 mV per step above code 1; `VRM12_MIN`, `VRM12_RES` in
    /// `micras_knc.c`). `None` for code 0, "unset".
    pub const fn vcore_mv(corevolt: u32) -> Option<u32> {
        let code = corevolt & 0xff;
        if code == 0 {
            None
        } else {
            Some(250 + 5 * (code - 1))
        }
    }

    /// Core clock in kHz from a 12-bit PLL ratio (COREFREQ or
    /// CURRENT_CLK_RATIO bits 11:0): feedback bits 8:1 times 200 MHz over a
    /// feed-forward divider of 1, 2 or 4 selected by bits 10:9 (inverted),
    /// scaled by 20 over the fused ICC divider (SCRATCH4 bits 29:25, 0 = 20).
    /// `None` for a code outside the PLL table (`ratio2freq`, `cpu_tab`:
    /// divider 1 takes feedback 8..16, dividers 2 and 4 take 8..15).
    pub const fn core_khz(ratio: u32, scratch4: u32) -> Option<u64> {
        let fwd = ((!ratio) >> 9) & 0x3;
        let bck = (ratio >> 1) & 0xff;
        let (div, max) = match fwd {
            0 => (1u64, 16),
            1 => (2, 15),
            2 => (4, 15),
            _ => return None,
        };
        if bck < 8 || bck > max {
            return None;
        }
        let icc = (scratch4 >> 25) & 0x1f;
        let icc = if icc == 0 { super::ICC_NOMINAL_DIVIDER as u64 } else { icc as u64 };
        Some(200_000 * bck as u64 / div * 20 / icc)
    }
}

// ---------------------------------------------------------------------------
// DMA engine

/// DMA configuration register: two bits per channel, bit `2n` = owner
/// (1 host, 0 card), bit `2n+1` = enable (`SBOX_DCR`, `md_mic_dma_enable_chan`
/// and `chan_to_dcr_mask` in MPSS `dma/mic_dma_md.c`).
pub const DCR: u32 = 0xA280;
/// Base of the per-channel DMA register blocks (`SBOX_DCAR_0`); channel `n`
/// is at `DMA_CHAN_BASE + DMA_CHAN_STRIDE * n`.
pub const DMA_CHAN_BASE: u32 = 0xA000;
/// Distance between channel blocks (`SBOX_DCAR_1 - SBOX_DCAR_0`).
pub const DMA_CHAN_STRIDE: u32 = 0x40;
/// Number of DMA channels (SSDG 2.1.8.2.1).
pub const DMA_CHAN_COUNT: u32 = 8;
/// Channel attribute register: interrupt masks and status (`SBOX_DCAR_n`).
pub const DCAR: u32 = 0x00;
/// Head pointer index, written by software (`SBOX_DHPR_n`, SSDG figure 2-11).
pub const DHPR: u32 = 0x04;
/// Tail pointer index, advanced by the engine (`SBOX_DTPR_n`).
pub const DTPR: u32 = 0x08;
/// Descriptor ring attributes, low 32 bits of the ring address (`SBOX_DRAR_LO_n`).
pub const DRAR_LO: u32 = 0x14;
/// Descriptor ring attributes, high part: bits 3:0 address bits 35:32,
/// bits 20:4 size in descriptors, bits 25:21 SMPT page, bit 26 SYS
/// (`SBOX_DRAR_HI_n`; `size_to_drar_hi_size`, `addr_to_drar_hi_smpt_bits`,
/// `SBOX_DRARHI_SYS_MASK` in `mic_dma_md.c`).
pub const DRAR_HI: u32 = 0x18;
/// Channel status (`SBOX_DSTAT_n`; completion count in bits 15:0,
/// `md_mic_dma_chan_read_dstat`).
pub const DSTAT: u32 = 0x20;
/// Channel error register (`SBOX_DCHERR_n`).
pub const DCHERR: u32 = 0x2C;
/// Channel error mask (`SBOX_DCHERRMSK_n`).
pub const DCHERRMSK: u32 = 0x30;
/// DCAR: mask the APIC (card) interrupt (`SBOX_DCAR_IM0`).
pub const DCAR_IM0: u32 = 1 << 24;
/// DCAR: mask the MSI-X (host) interrupt (`SBOX_DCAR_IM1`).
pub const DCAR_IM1: u32 = 1 << 25;
/// DRAR_HI: the ring lives in system (host) memory (`SBOX_DRARHI_SYS_MASK`).
pub const DRAR_HI_SYS: u32 = 1 << 26;

/// SBOX offset of register `reg` (one of the `D*` offsets above) of DMA channel `n`.
pub const fn dma_reg(n: u32, reg: u32) -> u32 {
    assert!(n < DMA_CHAN_COUNT);
    DMA_CHAN_BASE + DMA_CHAN_STRIDE * n + reg
}

/// Encode a memcpy descriptor (`md_mic_dma_memcpy_desc`, the `memcopy`
/// member of `union md_mic_dma_desc` in MPSS `include/mic/mic_dma_md.h`):
/// quadword 0 holds the 40-bit source address in bits 39:0 and the length
/// in 64-byte lines in bits 59:46; quadword 1 the 40-bit destination in
/// bits 39:0 and type 1 in bits 63:60. Addresses and length must be
/// multiples of 64 and the length at most `MIC_MAX_DMA_XFER_SIZE`
/// (1 MiB minus one line, 2^14 - 1 lines); the assertion enforces this.
pub const fn dma_memcpy_desc(src: u64, dst: u64, len: u64) -> (u64, u64) {
    assert!(src.is_multiple_of(64) && dst.is_multiple_of(64) && len.is_multiple_of(64) && len > 0 && len < (1 << 20));
    let mask40 = (1u64 << 40) - 1;
    ((src & mask40) | ((len / 64) << 46), (dst & mask40) | (1u64 << 60))
}

/// Encode a status descriptor (`md_mic_dma_prep_status_desc`, `status`
/// member): the engine writes the 64-bit `data` to the 40-bit card address
/// `dst` when it reaches the descriptor, after everything before it in the
/// ring. Type 2 in bits 63:60 of quadword 1; the interrupt bit (56) stays
/// clear. `dst` must be 8-byte aligned.
pub const fn dma_status_desc(data: u64, dst: u64) -> (u64, u64) {
    assert!(dst.is_multiple_of(8));
    (data, (dst & ((1u64 << 40) - 1)) | (2u64 << 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_offsets_match_intel_defines() {
        // MIC_X100_SBOX_APICICR7 is defined literally as 0xAA08 in mic_x100.h.
        assert_eq!(apicicr(7), APICICR7);
        // MIC_X100_DOWNLOAD_INFO is spad 2, MIC_X100_FW_SIZE is spad 5,
        // SBOX_SCRATCH4 is 0xAB30 in micsboxdefine.h.
        assert_eq!(spad(SPAD_DOWNLOAD_INFO), 0xAB28);
        assert_eq!(spad(SPAD_PLATFORM_INFO), 0xAB30);
        assert_eq!(spad(SPAD_FW_SIZE), 0xAB34);
        assert_eq!(smpt(31), 0x317C);
        assert_eq!(rdmasr(7), 0xB19C);
        // SBOX_DTPR_0 = 0xA008, SBOX_DCAR_1 = 0xA040, SBOX_DCHERRMSK_0 = 0xA030.
        assert_eq!(dma_reg(0, DTPR), 0xA008);
        assert_eq!(dma_reg(1, DCAR), 0xA040);
        assert_eq!(dma_reg(0, DCHERRMSK), 0xA030);
        assert_eq!(dma_reg(7, DCHERRMSK), 0xA1F0);
    }

    #[test]
    fn every_register_lies_inside_bar4() {
        for off in [
            spad(SPAD_COUNT - 1),
            APICICR7 + 4,
            RGCR,
            SICR0,
            SICE0,
            SICC0,
            SIAC0,
            MXAR0,
            MSIXPBACR,
            smpt(SMPT_COUNT - 1),
            rdmasr(RDMASR_COUNT - 1),
            SDBIC0,
            CURRENT_CLK_RATIO,
            COREFREQ,
            COREVOLT,
            ELAPSED_TIME_LOW,
            ELAPSED_TIME_HIGH,
            THERMAL_STATUS,
            STATUS_FAN2,
            BOARD_TEMP1,
            BOARD_TEMP2,
            BOARD_VOLTAGE_SENSE,
            CURRENT_DIE_TEMP0 + 8,
            MAX_DIE_TEMP0 + 8,
            DCR,
            dma_reg(DMA_CHAN_COUNT - 1, DCHERRMSK),
        ] {
            assert!(off.is_multiple_of(4), "offset {off:#x} is not dword aligned");
            assert!(u64::from(SBOX_BASE + off) + 4 <= MMIO_BAR_SIZE, "offset {off:#x} outside BAR4");
        }
        assert!(u64::from(POSTCODE) < u64::from(SBOX_BASE), "POSTCODE is a DBOX-side offset");
    }

    #[test]
    fn download_info_decodes_fields() {
        // Measured at first contact 2026-09-13 (docs/results/2026-09-13-first-contact.md):
        // spad2 = 0x040001c1, ready, BSP APIC ID 224, download address 64 MiB.
        let d = DownloadInfo(0x0400_01c1);
        assert!(d.ready());
        assert_eq!(d.apic_id(), 224);
        assert_eq!(d.download_addr(), 0x0400_0000);
        // Synthetic: a 9-bit APIC ID and the ready bit cleared.
        let raw = 0x0400_0000 | (0x1A3 << 1) | 1;
        assert_eq!(DownloadInfo(raw).apic_id(), 0x1A3);
        assert!(!DownloadInfo(raw & !1).ready());
    }

    #[test]
    fn platform_word_measured_on_this_card_decodes() {
        // First contact 2026-09-13: spad4 = 0x2800e6cf.
        let p = PlatformInfo(0x2800_e6cf);
        assert_eq!(p.thread_mask(), 0xF);
        assert_eq!(p.l2_kib(), 512);
        assert_eq!(p.memory_channels(), 12, "the 3120A has 12 GDDR channels");
        assert_eq!(p.icc_divider(), 20);
        assert_eq!(p.reference_mhz(), 200);
        assert!(!p.soft_reset());
        assert!(!p.internal_flash());
        // An unfused divider (0) means the nominal 20, as in micras_knc.c.
        assert_eq!(PlatformInfo(0).reference_mhz(), 200);
        assert_eq!(PlatformInfo(3 << 4).l2_kib(), 256);
        assert!(PlatformInfo(1 << 30).soft_reset());
        assert!(PlatformInfo(1 << 31).internal_flash());
    }

    #[test]
    fn clock_ratio_measured_on_this_card_decodes() {
        // 2026-09-17 on the card: CURRENTRATIO (SBOX+0x402C) = 0x04160416,
        // COREFREQ = 0x80010416, spad4 = 0x2800e6cf (docs/spec/sbox-registers.md).
        let p = PlatformInfo(0x2800_e6cf);
        let r = ClockRatio(0x0416_0416);
        assert_eq!(r.code(), 0x416);
        assert_eq!(r.goal_code(), 0x416);
        assert_eq!(r.feedback(), 11);
        assert_eq!(r.feedforward_code(), 2);
        assert_eq!(r.feedforward_div(), 2);
        assert_eq!(r.core_mhz(p), 1100, "200 MHz * 11 / 2");
        assert_eq!(
            ClockRatio(0x8001_0416).core_mhz(p),
            1100,
            "COREFREQ has the same ratio in bits 11:0"
        );
        assert_eq!(sensors::core_khz(0x8001_0416 & 0xfff, 0x2800_e6cf), Some(1_100_000));
        assert_eq!(sensors::core_khz(0, 0), None, "the KNF offset reads 0 and decodes to nothing");
    }

    #[test]
    fn the_two_clock_decoders_agree_inside_the_pll_table() {
        // cpu_tab in micras_knc.c: feed-forward code 3 (div 1) takes feedback
        // 8..16, codes 2 (div 2) and 1 (div 4) take 8..15; code 0 is not in the table.
        let p = PlatformInfo(20 << 25);
        for (code, max) in [(3u32, 16u32), (2, 15), (1, 15)] {
            for fb in 8..=max {
                let ratio = (code << 9) | (fb << 1);
                let mhz = ClockRatio(ratio).core_mhz(p);
                assert_eq!(sensors::core_khz(ratio, p.0), Some(u64::from(mhz) * 1000), "ratio {ratio:#x}");
            }
            assert_eq!(sensors::core_khz((code << 9) | (7 << 1), p.0), None);
            assert_eq!(sensors::core_khz((code << 9) | ((max + 1) << 1), p.0), None);
        }
        assert_eq!(sensors::core_khz(8 << 1, p.0), None, "feed-forward code 0");
        // A fused divider of 16 raises the reference to 250 MHz.
        assert_eq!(sensors::core_khz((2 << 9) | (11 << 1), 16 << 25), Some(1_375_000));
    }

    #[test]
    fn sensors_decode_measured_and_synthetic_words() {
        // hwmon "raw" on the card 2026-09-17: die 0x0340d837 0x0300d431 0x00000035,
        // corevolt 0xab, thermal_status 0xf0, board_temp1/2 and fan2 0
        // (docs/results/2026-09-16-sensors.md: seven fused sensors, two read 0).
        assert_eq!(
            sensors::die_temps([0x0340_d837, 0x0300_d431, 0x0000_0035]),
            [55, 54, 52, 49, 53, 48, 53, 0, 0]
        );
        assert_eq!(sensors::vcore_mv(0xab), Some(1100));
        assert_eq!(sensors::tmu_temp(0xf0), None);
        assert_eq!(sensors::board_temps(0), (None, None));
        assert_eq!(sensors::vddg_temp(0), 0);
        // Synthetic words exercising every field and valid bit.
        let t = sensors::die_temps([(50 << 20) | (49 << 10) | 48, 0, 0]);
        assert_eq!((t[0], t[1], t[2], t[3]), (48, 49, 50, 0));
        assert_eq!(sensors::board_temps((1 << 15) | 31 | (1 << 31) | (40 << 16)), (Some(31), Some(40)));
        assert_eq!(sensors::board_temps(31 | (40 << 16)), (None, None));
        assert_eq!(sensors::vddg_temp(0x5A << 12), 0x5A);
        assert_eq!(sensors::tmu_temp((1 << 31) | (66 << 22)), Some(66));
        assert_eq!(sensors::vcore_mv(0), None);
        assert_eq!(sensors::vcore_mv(0x01), Some(250));
        assert_eq!(sensors::vcore_mv(0x9b), Some(250 + 5 * 154));
        assert_eq!(sensors::vcore_mv(0xff), Some(1520), "VRM12_MAX");
    }

    #[test]
    fn dma_descriptor_layouts() {
        // 512 KiB from card 0x10126000 to host page 0 at IOVA 0x10100000.
        let (q0, q1) = dma_memcpy_desc(0x1012_6000, 0x80_1010_0000, 512 * 1024);
        assert_eq!(q0 & ((1 << 40) - 1), 0x1012_6000);
        assert_eq!((q0 >> 40) & 0x3f, 0, "index and reserved bits clear");
        assert_eq!((q0 >> 46) & 0x3fff, 8192);
        assert_eq!(q0 >> 60, 0);
        assert_eq!(q1 & ((1 << 40) - 1), 0x80_1010_0000);
        assert_eq!((q1 >> 40) & 0xfffff, 0, "twb, intr, c, co, ecy clear");
        assert_eq!(q1 >> 60, 1);
        // The largest legal transfer fills the 14-bit line count.
        let (q0, _) = dma_memcpy_desc(0, 0, (1 << 20) - 64);
        assert_eq!((q0 >> 46) & 0x3fff, 0x3fff);
        // Status descriptor: data verbatim, destination plus type 2.
        let (s0, s1) = dma_status_desc(0xdead_beef_0000_0001, 0x80_1010_1000);
        assert_eq!(s0, 0xdead_beef_0000_0001);
        assert_eq!(s1, 0x80_1010_1000 | (2 << 60));
    }
}
