//! The card as an object.
//!
//! [`Card`] owns the VFIO handles and the two BAR mappings and exposes the
//! operations the host needs: read POST code and scratchpads, reset, copy
//! bytes into card memory, send the boot interrupt. [`boot`] composes them
//! into the image-loading sequence Intel's driver used, with validation
//! before any byte touches the card.

#![warn(missing_docs)]

pub mod boot;
pub mod card;
pub mod dma;
pub mod ringmem;

pub use card::Card;

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// VFIO layer.
    #[error(transparent)]
    Vfio(#[from] phi_vfio::Error),
    /// Ring layer.
    #[error(transparent)]
    Ring(#[from] phi_ring::Error),
    /// The kernel image failed validation.
    #[error("kernel image: {0}")]
    Image(#[from] phi_regs::bootparams::BzImageError),
    /// The bootstrap is not in the ready state.
    #[error("bootstrap not ready: POST code {0:?}, SPAD2 {1:#010x} (reset the card first)")]
    NotReady(String, u32),
    /// A loader address or size is out of range.
    #[error("{0}")]
    Range(String),
    /// I/O on host files.
    #[error("{0}: {1}")]
    Io(&'static str, #[source] std::io::Error),
}

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;
