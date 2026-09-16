//! SBOX and DBOX register offsets.
//!
//! The card exposes two BARs. BAR4 ("MEMBAR1", 128 KiB) holds the DBOX
//! registers in its first 64 KiB and the SBOX registers in its second 64 KiB
//! (SSDG 2.1.12). Every constant in [`sbox`](self) that is documented as an
//! "SBOX offset" is relative to [`SBOX_BASE`]; add the two to get a BAR4
//! offset. The one DBOX-side register this project reads, [`POSTCODE`], is
//! a raw BAR4 offset.
//!
//! Sources: `mic_x100.h` unless stated otherwise.

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
/// SBOX base (`mic_x100_get_postcode`).
pub const POSTCODE: u32 = 0x242c;

/// First scratchpad register (`MIC_X100_SBOX_SPAD0`). SBOX offset.
pub const SPAD0: u32 = 0xAB20;
/// Number of scratchpad registers the driver model assumes (SPAD0..SPAD15).
pub const SPAD_COUNT: u32 = 16;

/// Scratchpad index carrying download info (`MIC_X100_DOWNLOAD_INFO`).
pub const SPAD_DOWNLOAD_INFO: u32 = 2;
/// Scratchpad index that receives the firmware image size (`MIC_X100_FW_SIZE`).
pub const SPAD_FW_SIZE: u32 = 5;

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

/// Reset control register (`MIC_X100_SBOX_RGCR`). Setting bit 0 resets the
/// card back to its bootstrap (`mic_x100_hw_reset`).
pub const RGCR: u32 = 0x4010;
/// Bit in [`RGCR`] that triggers the reset.
pub const RGCR_RESET: u32 = 1;

/// System interrupt cause register 0 (`MIC_X100_SBOX_SICR0`).
pub const SICR0: u32 = 0x9004;
/// System interrupt enable register 0 (`MIC_X100_SBOX_SICE0`).
pub const SICE0: u32 = 0x900C;
/// System interrupt clear register 0 (`MIC_X100_SBOX_SICC0`).
pub const SICC0: u32 = 0x9010;
/// System interrupt auto-clear register 0 (`MIC_X100_SBOX_SIAC0`).
pub const SIAC0: u32 = 0x9014;
/// MSI-X address register 0 (`MIC_X100_SBOX_MXAR0`).
pub const MXAR0: u32 = 0x9044;
/// MSI-X pending-bit-array control (`MIC_X100_SBOX_MSIXPBACR`).
pub const MSIXPBACR: u32 = 0x9084;
/// Doorbell bits (3:0) in SICR0/SICE0/SICC0/SIAC0 (`MIC_X100_SBOX_DBR_BITS`).
pub const SI_DOORBELL_MASK: u32 = 0xF;
/// DMA bits (15:8) in the same registers (`MIC_X100_SBOX_DMA_BITS`).
pub const SI_DMA_MASK: u32 = 0xFF << 8;

/// First system memory page table entry (`MIC_X100_SBOX_SMPT00`, `SBOX_SMPT00` in `intelmic.c`).
pub const SMPT00: u32 = 0x3100;
/// Number of SMPT entries (`mic_x100_smpt_hw_init`: `num_reg = 32`).
pub const SMPT_COUNT: u32 = 32;

/// First remote DMA status register, used as card-to-host doorbell (`MIC_X100_SBOX_RDMASR0`).
pub const RDMASR0: u32 = 0xB180;
/// Number of RDMASR registers (`MIC_X100_NUM_RDMASR_IRQ`).
pub const RDMASR_COUNT: u32 = 8;
/// System doorbell interrupt control 0 (`MIC_X100_SBOX_SDBIC0`).
pub const SDBIC0: u32 = 0xCC90;

/// SBOX offset of scratchpad `n` (panics if `n >= SPAD_COUNT`).
pub const fn spad(n: u32) -> u32 {
    assert!(n < SPAD_COUNT);
    SPAD0 + 4 * n
}

/// SBOX offset of the low dword of interrupt command register `n` (0..8).
pub const fn apicicr(n: u32) -> u32 {
    assert!(n < 8);
    APICICR0 + 8 * n
}

/// SBOX offset of SMPT entry `n` (0..32).
pub const fn smpt(n: u32) -> u32 {
    assert!(n < SMPT_COUNT);
    SMPT00 + 4 * n
}

/// SBOX offset of RDMASR register `n` (0..8).
pub const fn rdmasr(n: u32) -> u32 {
    assert!(n < RDMASR_COUNT);
    RDMASR0 + 4 * n
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_offsets_match_mainline_defines() {
        // MIC_X100_SBOX_APICICR7 is defined literally as 0xAA08 in mic_x100.h.
        assert_eq!(apicicr(7), APICICR7);
        // MIC_X100_DOWNLOAD_INFO is spad 2, MIC_X100_FW_SIZE is spad 5.
        assert_eq!(spad(SPAD_DOWNLOAD_INFO), 0xAB28);
        assert_eq!(spad(SPAD_FW_SIZE), 0xAB34);
        assert_eq!(smpt(31), 0x317C);
        assert_eq!(rdmasr(7), 0xB19C);
    }

    #[test]
    fn download_info_decodes_fields() {
        // addr 64 MiB, apic id 0x1A3 (max 9 bits), ready.
        let raw = 0x0400_0000 | (0x1A3 << 1) | 1;
        let d = DownloadInfo(raw);
        assert!(d.ready());
        assert_eq!(d.apic_id(), 0x1A3);
        assert_eq!(d.download_addr(), 0x0400_0000);
        assert!(!DownloadInfo(raw & !1).ready());
    }

    #[test]
    fn everything_fits_in_bar4() {
        for off in [
            SPAD0 + 4 * (SPAD_COUNT - 1),
            APICICR7 + 4,
            RGCR,
            SICR0,
            SIAC0,
            MXAR0,
            MSIXPBACR,
            smpt(31),
            rdmasr(7),
            SDBIC0,
        ] {
            assert!(((SBOX_BASE + off) as u64) < MMIO_BAR_SIZE, "offset {off:#x} outside BAR4");
        }
        assert!((POSTCODE as u64) < SBOX_BASE as u64, "POSTCODE is a DBOX-side offset");
    }
}

/// Current core clock ratio register (`MIC_SBOX_CURRENT_CLK_RATIO` in
/// Intel's `intelmic.c`, KNC value). SBOX offset.
pub const CURRENT_CLK_RATIO: u32 = 0x3004;
/// Core frequency register (`SBOX_COREFREQ`, `CONFIG_MK1OM` branch of `micsboxdefine.h`). SBOX offset.
pub const COREFREQ: u32 = 0x4100;
/// Core voltage register (`SBOX_COREVOLT`, KNC). SBOX offset.
pub const COREVOLT: u32 = 0x4104;
/// Elapsed time counter, low and high halves (`SBOX_ELAPSED_TIME_LOW/HIGH`, `micsboxdefine.h`). SBOX offsets.
pub const ELAPSED_TIME_LOW: u32 = 0x1074;
/// High half of the elapsed time counter.
pub const ELAPSED_TIME_HIGH: u32 = 0x1078;
/// Thermal status register (`SBOX_THERMAL_STATUS`, `micsboxdefine.h`). SBOX offset.
pub const THERMAL_STATUS: u32 = 0x1018;
/// Scratchpad index holding the bootstrap's platform word (`MIC_SBOX_SCRATCH4` in `intelmic.c`).
pub const SPAD_PLATFORM_INFO: u32 = 4;
/// The PLL reference the ICC divider divides, in MHz (`CORE_VCO` in `intelmic.c`).
pub const CORE_VCO_MHZ: u32 = 4000;

/// Decoded scratchpad 4, the bootstrap's platform description word
/// (`sboxScratch4RegDef` in Intel's `intelmic.c`).
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
    /// ICC divider (bits 29:25); the reference clock is 4000 MHz divided by it.
    pub const fn icc_divider(self) -> u32 {
        (self.0 >> 25) & 0x1F
    }
    /// Reference clock in MHz, or 0 if the divider is 0.
    pub const fn reference_mhz(self) -> u32 {
        match CORE_VCO_MHZ.checked_div(self.icc_divider()) {
            Some(v) => v,
            None => 0,
        }
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

/// Decoded [`CURRENT_CLK_RATIO`] (`mclkRatioEncoding` in Intel's `intelmic.c`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockRatio(pub u32);

impl ClockRatio {
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
    /// Core frequency in MHz given the platform word (`get_core_freq` in `intelmic.c`).
    pub const fn core_mhz(self, platform: PlatformInfo) -> u32 {
        platform.reference_mhz() * self.feedback() / self.feedforward_div()
    }
}

#[cfg(test)]
mod platform_tests {
    use super::*;

    #[test]
    fn spad4_measured_on_this_card_decodes() {
        // First contact 2026-09-13: spad4 = 0x2800e6cf (docs/results).
        let p = PlatformInfo(0x2800_e6cf);
        assert_eq!(p.thread_mask(), 0xF);
        assert_eq!(p.l2_kib(), 512);
        assert_eq!(p.memory_channels(), 12, "the 3120A has 12 GDDR channels");
        assert_eq!(p.icc_divider(), 20);
        assert_eq!(p.reference_mhz(), 200);
        assert!(!p.soft_reset());
        assert!(!p.internal_flash());
        // fb = 11, ff code 2 (div 2): 200 * 11 / 2 = 1100 MHz, the 3120A's clock.
        let r = ClockRatio((11 << 1) | (2 << 9));
        assert_eq!(r.core_mhz(p), 1100);
    }
}

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
/// Channel status (`SBOX_DSTAT_n`; completion count in bits 15:0).
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

/// Encode a memcpy descriptor (`md_mic_dma_memcpy_desc`, `union
/// md_mic_dma_desc` in MPSS `include/mic/mic_dma_md.h`): quadword 0 holds
/// the 40-bit source address and the length in 64-byte lines in bits 59:46;
/// quadword 1 the 40-bit destination and type 1 in bits 63:60. Addresses
/// and length must be multiples of 64; the length at most 2^14 - 1 lines.
pub const fn dma_memcpy_desc(src: u64, dst: u64, len: u64) -> (u64, u64) {
    assert!(src.is_multiple_of(64) && dst.is_multiple_of(64) && len.is_multiple_of(64) && len > 0 && len < (1 << 20));
    let mask40 = (1u64 << 40) - 1;
    ((src & mask40) | ((len / 64) << 46), (dst & mask40) | (1u64 << 60))
}

#[cfg(test)]
mod dma_tests {
    use super::*;

    #[test]
    fn descriptor_layout() {
        // 512 KiB from card 0x10126000 to host page 0 at IOVA 0x10100000.
        let (q0, q1) = dma_memcpy_desc(0x1012_6000, 0x80_1010_0000, 512 * 1024);
        assert_eq!(q0 & ((1 << 40) - 1), 0x1012_6000);
        assert_eq!((q0 >> 46) & 0x3fff, 8192);
        assert_eq!(q1 & ((1 << 40) - 1), 0x80_1010_0000);
        assert_eq!(q1 >> 60, 1);
        assert_eq!(dma_reg(0, DTPR), 0xA008);
        assert_eq!(dma_reg(1, DCAR), 0xA040);
    }
}
