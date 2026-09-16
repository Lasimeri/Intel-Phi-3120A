//! Wire layout, byte-for-byte as in `docs/spec/ring-protocol.md`.
//!
//! Field offsets are written as constants rather than read through struct
//! fields because the data lives in device memory and is accessed through
//! [`crate::RingMemory`], never through a Rust reference.

/// `'P','H','I','R'` little-endian.
pub const REGION_MAGIC: u32 = u32::from_le_bytes(*b"PHIR");
/// `'R','I','N','G'` little-endian.
pub const RING_MAGIC: u32 = u32::from_le_bytes(*b"RING");
/// Protocol version this crate implements.
pub const VERSION: u32 = 1;

/// Size of `phi_region_hdr` in bytes.
pub const REGION_HDR_SIZE: usize = 64;
/// Size of one `phi_channel_desc`.
pub const CHANNEL_DESC_SIZE: usize = 64;
/// Size of the `phi_ring` header that precedes its data area (three
/// 64-byte lines: magic/size, head, tail).
pub const RING_HDR_SIZE: usize = 192;

/// Offsets inside `phi_region_hdr`.
pub mod region_hdr {
    /// u32 magic.
    pub const MAGIC: usize = 0;
    /// u32 version.
    pub const VERSION: usize = 4;
    /// u32 total region size in bytes.
    pub const REGION_SIZE: usize = 8;
    /// u32 number of channel descriptors following the header.
    pub const CHANNEL_COUNT: usize = 12;
    /// u64 host wall clock (nanoseconds since the Unix epoch) at boot.
    pub const HOST_EPOCH_NS: usize = 16;
    /// u64 flags written by the card (bit 0: kernel reached init).
    pub const CARD_BOOT_FLAGS: usize = 24;
}

/// Offsets inside `phi_channel_desc`.
pub mod channel_desc {
    /// u32 kind (see [`super::ChannelKind`]).
    pub const KIND: usize = 0;
    /// u32 flags (reserved, 0).
    pub const FLAGS: usize = 4;
    /// u32 region offset of the host-to-card ring.
    pub const H2C_OFFSET: usize = 8;
    /// u32 data size of the host-to-card ring.
    pub const H2C_SIZE: usize = 12;
    /// u32 region offset of the card-to-host ring.
    pub const C2H_OFFSET: usize = 16;
    /// u32 data size of the card-to-host ring.
    pub const C2H_SIZE: usize = 20;
    /// u32 region offset of the channel's data area (0 = none): bounce
    /// slots for the block channel, 4096-byte aligned.
    pub const DATA_OFFSET: usize = 24;
    /// u32 size of the data area in bytes.
    pub const DATA_SIZE: usize = 28;
}

/// Offsets inside `phi_ring`, relative to the ring's start.
pub mod ring_hdr {
    /// u32 magic.
    pub const MAGIC: usize = 0;
    /// u32 data size (power of two).
    pub const SIZE: usize = 4;
    /// u32 producer index, on its own cache line.
    pub const HEAD: usize = 64;
    /// u32 consumer index, on its own cache line.
    pub const TAIL: usize = 128;
    /// Start of the data area.
    pub const DATA: usize = 192;
}

/// Channel kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ChannelKind {
    /// Raw bytes: the card's console.
    Console = 1,
    /// Length-prefixed Ethernet frames.
    Network = 2,
    /// Framed rpc messages for the host tool (phi-rpc): commands and files.
    Rpc = 3,
    /// Block device: 32-byte request records from the card, 8-byte
    /// completions from the host (kernel patch 0025, phictl boot --disk).
    Block = 4,
}

impl ChannelKind {
    /// Decode from the wire value.
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::Console),
            2 => Some(Self::Network),
            3 => Some(Self::Rpc),
            4 => Some(Self::Block),
            _ => None,
        }
    }
}

/// Bit in `card_boot_flags` the card sets when its kernel reached `init`.
pub const CARD_FLAG_INIT_REACHED: u64 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_spec() {
        assert_eq!(REGION_MAGIC, 0x5249_4850);
        assert_eq!(RING_MAGIC, 0x474E_4952);
        assert_eq!(ring_hdr::DATA, RING_HDR_SIZE);
        // Head and tail on distinct 64-byte lines, data on a fresh line.
        assert_eq!(ring_hdr::HEAD % 64, 0);
        assert_eq!(ring_hdr::TAIL % 64, 0);
        assert_eq!(ring_hdr::DATA % 64, 0);
        assert_eq!(REGION_HDR_SIZE % 64, 0);
        assert_eq!(CHANNEL_DESC_SIZE % 64, 0);
    }
}
