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
