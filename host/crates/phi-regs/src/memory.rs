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
/// bit 0 is no-snoop). Layout from `BUILD_SMPT` in `intelmic.c` and
/// `mic_x100_smpt_set` in `mic_x100.c`. The page number is truncated to
/// 30 bits; host addresses are at most 48 bits, so nothing is lost.
pub const fn smpt_entry(host_addr: u64, no_snoop: bool) -> u32 {
    let page = (host_addr >> SMPT_PAGE_SHIFT) as u32;
    (page << 2) | (no_snoop as u32)
}

/// Card physical address at which the bootstrap asks for the kernel image
/// on this card (64 MiB, `SPAD2` at first contact 2026-09-13, and the
/// value the `mic_x100_load_ramdisk` comment calls typical). The loader
/// reads the live value from `SPAD2`; this constant only documents the
/// layout the ring region default is chosen against.
pub const TYPICAL_DOWNLOAD_ADDR: u64 = 64 * 1024 * 1024;

/// Default card physical address of the ring transport region
/// (`docs/spec/ring-protocol.md`): 256 MiB, above the 64 MiB download
/// address and the 128 MiB initramfs slot (`phi-hw` places the initramfs
/// at twice the download address, as Intel's loader did).
pub const RING_REGION_BASE: u64 = 0x1000_0000;
/// Default size of the ring transport region: 16 MiB since 2026-09-16, when
/// the block channel's bounce slots (8 MiB) joined the console, network and
/// rpc rings; `phictl boot --ring-size` overrides it. The card kernel
/// reserves whatever size the command line names.
pub const RING_REGION_BYTES: u64 = 16 * 1024 * 1024;

// Compile-time layout check: the ring region must sit above the initramfs
// slot (twice the download address) and inside the card GDDR. It was first
// placed at 32 MiB, below the download address; bulk writes there reset
// the host (`docs/results/2026-09-13-p3-kernel-build.md`).
const _: () = assert!(RING_REGION_BASE >= 2 * TYPICAL_DOWNLOAD_ADDR);
const _: () = assert!(RING_REGION_BASE + RING_REGION_BYTES <= GDDR_BYTES_3120A);

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
        // Identity map of the 32 pages: entry n holds page n (smpt_identity in phi-hw).
        assert_eq!(smpt_entry(31 << 34, false), 31 << 2);
        assert_eq!(smpt_entry(0, false), 0);
    }

    #[test]
    fn card_map_is_consistent() {
        const { assert!(GDDR_BYTES_3120A < APERTURE_BYTES) };
        assert_eq!(SMPT_PAGE_BYTES, 16 * 1024 * 1024 * 1024);
        assert_eq!(SMPT_BASE, 512 * 1024 * 1024 * 1024, "system range starts at 512 GiB");
        assert_eq!(SBOX_PHYS - DBOX_PHYS, 0x1_0000, "DBOX and SBOX are adjacent 64 KiB blocks");
        assert_eq!(RING_REGION_BASE, 256 * 1024 * 1024);
    }
}
