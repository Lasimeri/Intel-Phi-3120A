//! The card's 512-bit vector unit, for Rust running on the card.
//!
//! Bindings to `libknc` (`card/lib/knc/knc.md`), whose kernels are
//! hand-encoded MVEX: rustc cannot emit Knights Corner vector instructions
//! any more than clang can, and `core::arch` has no intrinsics for them,
//! so the only route from Rust to this vector unit is through that
//! library. See lib.md.
//!
//! ```ignore
//! use knc::{Block, Packed, Codec};
//!
//! let codec = Codec::new(11).unwrap();
//! let mut values = Block::zeroed();
//! let mut packed = Packed::zeroed();
//! let mut out = Block::zeroed();
//!
//! for (i, v) in values.iter_mut().enumerate() {
//!     *v = (i as i32) & 0x7ff;
//! }
//! codec.pack(&mut packed, &values);
//! codec.unpack(&mut out, &packed);
//! assert_eq!(out.as_slice(), values.as_slice());
//! ```
//!
//! The layout is not a contiguous bitstream: value `i` lives in lane
//! `i % 16` at position `i / 16`. `knc.md` explains why, and packed bytes
//! produced here are not interchangeable with an ordinary packed stream.

#![no_std]

use core::fmt;

/// Values in one block. Fixed by the layout: sixteen lanes of 64 values.
pub const BLOCK: usize = 1024;

/// The widest packed block, in 32-bit words: `BLOCK` values of 32 bits.
const MAX_WORDS: usize = BLOCK;

unsafe extern "C" {
    fn knc_unpack(out: *mut i32, packed: *const u32, bits: u32);
    fn knc_pack(packed: *mut u32, values: *const i32, bits: u32);
    fn knc_memcpy64(dst: *mut u8, src: *const u8, blocks: usize) -> *mut u8;
}

/// A width outside 1 to 32.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidthError(pub u32);

impl fmt::Display for WidthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "knc: bit width {} is not in 1 to 32", self.0)
    }
}

impl core::error::Error for WidthError {}

/// One block of unpacked values, aligned as the kernels require.
///
/// The alignment is the reason this type exists rather than a plain
/// `[i32; BLOCK]`: every kernel loads and stores whole 64-byte vectors and
/// there is no unaligned form to fall back to.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct Block([i32; BLOCK]);

/// One packed block. Always the size of the widest one (4 KiB); a narrower
/// width uses the first `BLOCK * bits / 32` words and leaves the rest.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct Packed([u32; MAX_WORDS]);

macro_rules! aligned_block {
    ($t:ident, $elem:ty, $len:expr) => {
        impl $t {
            /// All zeroes.
            pub const fn zeroed() -> Self {
                $t([0; $len])
            }

            pub fn as_slice(&self) -> &[$elem] {
                &self.0
            }

            pub fn as_mut_slice(&mut self) -> &mut [$elem] {
                &mut self.0
            }
        }

        impl Default for $t {
            fn default() -> Self {
                Self::zeroed()
            }
        }

        impl core::ops::Deref for $t {
            type Target = [$elem];
            fn deref(&self) -> &[$elem] {
                &self.0
            }
        }

        impl core::ops::DerefMut for $t {
            fn deref_mut(&mut self) -> &mut [$elem] {
                &mut self.0
            }
        }
    };
}

aligned_block!(Block, i32, BLOCK);
aligned_block!(Packed, u32, MAX_WORDS);

/// A kernel pair bound to one bit width.
///
/// The width is checked once, here, so the calls below cannot fail and do
/// not branch. `libknc`'s C entry point returns silently for a width
/// outside 1 to 32; this type makes that unreachable instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Codec {
    bits: u32,
}

impl Codec {
    /// `bits` must be 1 to 32.
    pub fn new(bits: u32) -> Result<Codec, WidthError> {
        if (1..=32).contains(&bits) {
            Ok(Codec { bits })
        } else {
            Err(WidthError(bits))
        }
    }

    pub fn bits(self) -> u32 {
        self.bits
    }

    /// Words of `Packed` this width actually uses.
    pub fn words(self) -> usize {
        BLOCK * self.bits as usize / 32
    }

    /// Bytes of `Packed` this width actually uses.
    pub fn packed_bytes(self) -> usize {
        self.words() * 4
    }

    /// One block of values into one packed block. Values wider than
    /// `bits` are truncated rather than corrupting their neighbours.
    pub fn pack(self, packed: &mut Packed, values: &Block) {
        // Safe: both are 64-byte aligned by construction, both are a full
        // block, and the width was checked in `new`.
        unsafe { knc_pack(packed.0.as_mut_ptr(), values.0.as_ptr(), self.bits) }
    }

    /// One packed block back into values.
    pub fn unpack(self, out: &mut Block, packed: &Packed) {
        unsafe { knc_unpack(out.0.as_mut_ptr(), packed.0.as_ptr(), self.bits) }
    }
}

/// Copy whole 64-byte blocks. Both slices must be 64-byte aligned and at
/// least `blocks * 64` bytes long; the alignment is checked here because,
/// unlike `Block`, an arbitrary slice carries no guarantee.
pub fn memcpy64(dst: &mut [u8], src: &[u8], blocks: usize) -> Result<(), &'static str> {
    let bytes = blocks * 64;

    if dst.len() < bytes || src.len() < bytes {
        return Err("knc::memcpy64: slice shorter than blocks * 64");
    }
    if dst.as_ptr() as usize % 64 != 0 || src.as_ptr() as usize % 64 != 0 {
        return Err("knc::memcpy64: both pointers must be 64-byte aligned");
    }
    unsafe { knc_memcpy64(dst.as_mut_ptr(), src.as_ptr(), blocks) };
    Ok(())
}
