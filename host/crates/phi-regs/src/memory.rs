//! Card memory map constants (SSDG 2.1.12, `intelmic.c`, measurements).

/// GDDR5 capacity of the 3120A in bytes (6 GB, Intel ARK). Card physical
/// addresses below this are backed by memory; the aperture BAR is larger.
pub const GDDR_BYTES_3120A: u64 = 6 * 1024 * 1024 * 1024;

/// Size of the aperture BAR (MEMBAR0) on this card, measured (`docs/hardware.md`).
pub const APERTURE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// Card physical address of the DBOX register block (SSDG 2.1.12).
pub const DBOX_PHYS: u64 = 0x08_007C_0000;
/// Card physical address of the SBOX register block (SSDG 2.1.12, `MIC_SBOX_BASE` in `intelmic.c`).
pub const SBOX_PHYS: u64 = 0x08_007D_0000;
/// Card physical address of the local APIC (SSDG 2.1.12).
pub const LAPIC_PHYS: u64 = 0x00_FEE0_0000;
/// Card physical address of the boot flash window (SSDG 2.1.12).
pub const FLASH_PHYS: u64 = 0x00_FF00_0000;

/// Card physical base of the system (host) address range (`mic_x100_smpt_hw_init`: `base = 0x8000000000`).
pub const SMPT_BASE: u64 = 0x80_0000_0000;
/// log2 of an SMPT page (`page_shift = 34`, 16 GiB).
pub const SMPT_PAGE_SHIFT: u32 = 34;
/// Size of one SMPT page.
pub const SMPT_PAGE_BYTES: u64 = 1 << SMPT_PAGE_SHIFT;

/// Encode an SMPT entry for a host address (bits 31:2 hold `addr >> 34`;
/// bit 0 is no-snoop). Layout from `BUILD_SMPT` in `intelmic.c`.
pub const fn smpt_entry(host_addr: u64, no_snoop: bool) -> u32 {
    let page = (host_addr >> SMPT_PAGE_SHIFT) as u32;
    (page << 2) | (no_snoop as u32)
}

/// Default card physical address of the ring transport region
/// (`docs/spec/ring-protocol.md`): 32 MiB, below the usual 64 MiB download
/// address reported by the bootstrap and above the low memory the bootstrap
/// uses for AP boot code.
pub const RING_REGION_BASE: u64 = 0x0200_0000;
/// Default size of the ring transport region.
pub const RING_REGION_BYTES: u64 = 1024 * 1024;

// Compile-time layout checks: the ring region must sit below the typical
// 64 MiB download address (comment in `mic_x100_load_ramdisk`) and inside
// the card GDDR.
const _: () = assert!(RING_REGION_BASE + RING_REGION_BYTES <= 64 * 1024 * 1024);
const _: () = assert!(RING_REGION_BASE + RING_REGION_BYTES < GDDR_BYTES_3120A);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smpt_entry_encoding() {
        // Host IOVA 16 GiB is page 1.
        assert_eq!(smpt_entry(1 << 34, false), 0b100);
        assert_eq!(smpt_entry(1 << 34, true), 0b101);
        // Bits below 34 do not leak into the entry.
        assert_eq!(smpt_entry((1 << 34) | 0xFFF, false), 0b100);
    }
}
