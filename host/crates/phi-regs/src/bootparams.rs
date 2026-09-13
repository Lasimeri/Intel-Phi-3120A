//! Linux x86 boot protocol: the pieces of the bzImage setup header that the
//! host loader reads and patches.
//!
//! The bootstrap builds its own `struct boot_params` (SSDG 2.2.4), so the
//! host only touches the header inside the kernel image it copies into card
//! memory: it validates the image and writes the initramfs location into
//! `ramdisk_image`/`ramdisk_size`, exactly as `mic_x100_load_ramdisk` did.
//! Offsets are from Linux `Documentation/arch/x86/boot.rst` and
//! `arch/x86/include/uapi/asm/bootparam.h`.

/// Offset of the boot sector signature `0xAA55` (little-endian at 0x1FE).
pub const OFF_BOOT_FLAG: usize = 0x1FE;
/// Expected value at [`OFF_BOOT_FLAG`].
pub const BOOT_FLAG: u16 = 0xAA55;
/// Number of 512-byte setup sectors after the boot sector (0 means 4).
pub const OFF_SETUP_SECTS: usize = 0x1F1;
/// The `"HdrS"` magic, present since boot protocol 2.00.
pub const OFF_HEADER: usize = 0x202;
/// Expected bytes at [`OFF_HEADER`].
pub const HEADER_MAGIC: [u8; 4] = *b"HdrS";
/// Boot protocol version, `0xMMmm` (major, minor).
pub const OFF_VERSION: usize = 0x206;
/// Load flags byte (bit 0 = LOADED_HIGH, bit 5 = QUIET_FLAG; KEEP_SEGMENTS,
/// bit 6, was removed from the protocol in Linux 5.15 and is ignored).
pub const OFF_LOADFLAGS: usize = 0x211;
/// Type of loader byte; `0xFF` means "undefined".
pub const OFF_TYPE_OF_LOADER: usize = 0x210;
/// 32-bit physical address of the initramfs.
pub const OFF_RAMDISK_IMAGE: usize = 0x218;
/// Size of the initramfs in bytes.
pub const OFF_RAMDISK_SIZE: usize = 0x21C;
/// 32-bit physical address of the command line.
pub const OFF_CMD_LINE_PTR: usize = 0x228;
/// Highest address the initramfs may occupy (protocol 2.03+).
pub const OFF_INITRD_ADDR_MAX: usize = 0x22C;
/// Maximum command line length without the trailing NUL (protocol 2.06+).
pub const OFF_CMDLINE_SIZE: usize = 0x238;
/// Extended load flags, protocol 2.12+ (bit 0 = XLF_KERNEL_64, bit 1 = XLF_CAN_BE_LOADED_ABOVE_4G).
pub const OFF_XLOADFLAGS: usize = 0x236;
/// Bytes the kernel needs from its load address during boot (protocol 2.10+).
pub const OFF_INIT_SIZE: usize = 0x260;

/// Minimum image length that contains every field above.
pub const MIN_IMAGE_LEN: usize = 0x264;

/// Facts extracted from a bzImage header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BzImageInfo {
    /// Boot protocol version, e.g. `0x020F` for 2.15.
    pub protocol: u16,
    /// Number of setup sectors (already normalized: 0 becomes 4).
    pub setup_sects: u8,
    /// `cmdline_size` from the header, or 255 for protocols before 2.06.
    pub cmdline_max: u32,
    /// `init_size`, or 0 if the header predates 2.10.
    pub init_size: u32,
    /// True if the header advertises a 64-bit entry (`XLF_KERNEL_64`).
    pub kernel_64: bool,
}

/// Errors from [`parse_bzimage`].
#[derive(Debug, PartialEq, Eq)]
pub enum BzImageError {
    /// Image shorter than the setup header.
    TooShort,
    /// Missing `0xAA55` boot flag.
    BadBootFlag,
    /// Missing `"HdrS"` magic.
    BadMagic,
    /// Boot protocol too old for a 32-bit entry with ramdisk fields (needs 2.03+).
    ProtocolTooOld(u16),
}

impl core::fmt::Display for BzImageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooShort => write!(f, "image is shorter than the setup header"),
            Self::BadBootFlag => write!(f, "boot flag 0xAA55 not found at 0x1FE"),
            Self::BadMagic => write!(f, "'HdrS' magic not found at 0x202"),
            Self::ProtocolTooOld(v) => write!(f, "boot protocol {:#06x} is older than 2.03", v),
        }
    }
}

impl std::error::Error for BzImageError {}

fn rd16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn rd32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// Validate a bzImage and extract the header facts the loader needs.
pub fn parse_bzimage(image: &[u8]) -> Result<BzImageInfo, BzImageError> {
    if image.len() < MIN_IMAGE_LEN {
        return Err(BzImageError::TooShort);
    }
    if rd16(image, OFF_BOOT_FLAG) != BOOT_FLAG {
        return Err(BzImageError::BadBootFlag);
    }
    if image[OFF_HEADER..OFF_HEADER + 4] != HEADER_MAGIC {
        return Err(BzImageError::BadMagic);
    }
    let protocol = rd16(image, OFF_VERSION);
    if protocol < 0x0203 {
        return Err(BzImageError::ProtocolTooOld(protocol));
    }
    let raw_sects = image[OFF_SETUP_SECTS];
    let setup_sects = if raw_sects == 0 { 4 } else { raw_sects };
    let cmdline_max = if protocol >= 0x0206 { rd32(image, OFF_CMDLINE_SIZE) } else { 255 };
    let init_size = if protocol >= 0x020A { rd32(image, OFF_INIT_SIZE) } else { 0 };
    let kernel_64 = protocol >= 0x020C && (rd16(image, OFF_XLOADFLAGS) & 1) != 0;
    Ok(BzImageInfo {
        protocol,
        setup_sects,
        cmdline_max,
        init_size,
        kernel_64,
    })
}

/// Little-endian bytes to write at [`OFF_RAMDISK_IMAGE`] and
/// [`OFF_RAMDISK_SIZE`] for a ramdisk at `addr` of `size` bytes.
pub fn ramdisk_fields(addr: u32, size: u32) -> ([u8; 4], [u8; 4]) {
    (addr.to_le_bytes(), size.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(protocol: u16) -> Vec<u8> {
        let mut v = vec![0u8; MIN_IMAGE_LEN + 512];
        v[OFF_BOOT_FLAG..OFF_BOOT_FLAG + 2].copy_from_slice(&BOOT_FLAG.to_le_bytes());
        v[OFF_HEADER..OFF_HEADER + 4].copy_from_slice(&HEADER_MAGIC);
        v[OFF_VERSION..OFF_VERSION + 2].copy_from_slice(&protocol.to_le_bytes());
        v[OFF_SETUP_SECTS] = 0; // exercises the "0 means 4" rule
        v[OFF_CMDLINE_SIZE..OFF_CMDLINE_SIZE + 4].copy_from_slice(&2047u32.to_le_bytes());
        v[OFF_INIT_SIZE..OFF_INIT_SIZE + 4].copy_from_slice(&0x0180_0000u32.to_le_bytes());
        v[OFF_XLOADFLAGS..OFF_XLOADFLAGS + 2].copy_from_slice(&1u16.to_le_bytes());
        v
    }

    #[test]
    fn parses_modern_header() {
        let info = parse_bzimage(&synthetic(0x020F)).unwrap();
        assert_eq!(info.protocol, 0x020F);
        assert_eq!(info.setup_sects, 4);
        assert_eq!(info.cmdline_max, 2047);
        assert_eq!(info.init_size, 0x0180_0000);
        assert!(info.kernel_64);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_bzimage(&[0u8; 16]), Err(BzImageError::TooShort));
        let mut v = synthetic(0x020F);
        v[OFF_BOOT_FLAG] = 0;
        assert_eq!(parse_bzimage(&v), Err(BzImageError::BadBootFlag));
        let mut v = synthetic(0x020F);
        v[OFF_HEADER] = b'X';
        assert_eq!(parse_bzimage(&v), Err(BzImageError::BadMagic));
        assert_eq!(parse_bzimage(&synthetic(0x0200)), Err(BzImageError::ProtocolTooOld(0x0200)));
    }

    #[test]
    fn ramdisk_fields_are_little_endian() {
        let (a, s) = ramdisk_fields(0x0800_0000, 0x0123_4567);
        assert_eq!(a, [0x00, 0x00, 0x00, 0x08]);
        assert_eq!(s, [0x67, 0x45, 0x23, 0x01]);
    }
}
