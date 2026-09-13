//! Host side of the ring transport specified in `docs/spec/ring-protocol.md`.
//!
//! The crate is hardware-independent: all access goes through the
//! [`RingMemory`] trait, implemented by an in-memory `Vec` for tests and by
//! the aperture mapping in `phi-hw` for the real card. Layouts are
//! `#[repr(C)]` and mirrored by `card/drivers/phinet/include/phi_ring.h`;
//! both sides assert the same sizes.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod layout;
pub mod memory;
pub mod region;
pub mod ring;

pub use layout::{ChannelKind, CHANNEL_DESC_SIZE, REGION_HDR_SIZE, REGION_MAGIC, RING_HDR_SIZE, RING_MAGIC, VERSION};
pub use memory::{RingMemory, VecMemory};
pub use region::{ChannelPlan, ChannelView, Region};
pub use ring::{Consumer, Producer};

/// Errors from region validation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// The region header magic is missing (card memory not formatted, or wrong base address).
    #[error("region magic mismatch: found {0:#010x}, expected {1:#010x}")]
    BadMagic(u32, u32),
    /// Unsupported protocol version.
    #[error("region version {0} not supported (this host speaks {1})")]
    BadVersion(u32, u32),
    /// A ring size is not a power of two or is zero.
    #[error("ring size {0:#x} is not a non-zero power of two")]
    BadRingSize(u32),
    /// The requested layout does not fit in the region.
    #[error("layout needs {0:#x} bytes but the region is {1:#x}")]
    DoesNotFit(usize, usize),
    /// No channel of the requested kind exists.
    #[error("no channel of kind {0:?}")]
    NoChannel(ChannelKind),
}
