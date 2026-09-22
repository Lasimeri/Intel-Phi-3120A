//! The host/card contract, mirrored from `card/vpu/vpu_proto.h`.
//!
//! The two files must agree byte for byte: the card reads what the host
//! writes at these offsets and nothing checks it at run time. Both carry
//! the same size assertions, the unit tests below pin every field offset,
//! and `tools/vpu-layout-check.c` compares the C side against the same
//! numbers.

/// Window offset of the card's readiness word.
pub const OFF_READY: usize = 0;
/// Window offset of the request descriptor.
pub const OFF_REQ: usize = 64;
/// Window offset of the reply descriptor.
pub const OFF_REPLY: usize = 256;
/// Where bulk data starts. Everything below is control words.
pub const OFF_DATA: u64 = 1 << 20;

/// What the card writes at `OFF_READY` while it is polling ("VPU_READ").
pub const MAGIC: u64 = 0x5650_555F_5245_4144;

/// Degree-30 Horner in float32: `out[i] = poly(in[i])`.
pub const K_POLY30: u32 = 1;

/// The card moves data with `O_DIRECT`, so every offset and every
/// transfer is a whole number of these.
pub const BLOCK: u64 = 4096;
/// The POLY30 kernel works in steps of this many elements.
pub const CHUNK: u64 = 128;

/// Round a byte count or offset up to a block boundary.
pub const fn round_up(x: u64) -> u64 {
    (x + BLOCK - 1) & !(BLOCK - 1)
}

/// `struct vpu_request`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// The host bumps this last; the card polls it.
    pub seq: u64,
    pub kernel: u32,
    /// How many card threads to spread the work across.
    pub threads: u32,
    /// Elements.
    pub n: u64,
    pub in_off: u64,
    pub out_off: u64,
    pub aux_off: u64,
    pub aux_len: u64,
}

/// `struct vpu_reply`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    /// Echoes the request's sequence number once the work is done.
    pub seq: u64,
    /// Time on the vector units alone.
    pub compute_ns: u64,
    /// From the doorbell being seen to the reply being written.
    pub total_ns: u64,
    /// Moving the input and coefficients onto the card.
    pub pull_ns: u64,
    /// Moving the output back.
    pub push_ns: u64,
    pub status: i32,
    /// Slices that actually ran.
    pub threads: i32,
}

pub const OK: i32 = 0;
pub const E_ALLOC: i32 = -1;
pub const E_PULL: i32 = -2;
pub const E_PUSH: i32 = -3;
pub const E_REQUEST: i32 = -4;
pub const E_KERNEL: i32 = -5;

/// A reply status as words.
pub fn status_name(status: i32) -> &'static str {
    match status {
        OK => "ok",
        E_ALLOC => "card could not reserve buffers",
        E_PULL => "card could not read the input from the window",
        E_PUSH => "card could not write the output (unaligned offset?)",
        E_REQUEST => "request rejected: n is zero or an offset is not block aligned",
        E_KERNEL => "unknown kernel number",
        _ => "unknown status",
    }
}

// The C header asserts the same two sizes.
const _: [(); 56] = [(); std::mem::size_of::<Request>()];
const _: [(); 48] = [(); std::mem::size_of::<Reply>()];

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    /// These are the numbers `tools/vpu-layout-check.c` prints for the C
    /// side. If either file changes, one of the two checks fails.
    #[test]
    fn request_layout_matches_the_c_header() {
        assert_eq!(offset_of!(Request, seq), 0);
        assert_eq!(offset_of!(Request, kernel), 8);
        assert_eq!(offset_of!(Request, threads), 12);
        assert_eq!(offset_of!(Request, n), 16);
        assert_eq!(offset_of!(Request, in_off), 24);
        assert_eq!(offset_of!(Request, out_off), 32);
        assert_eq!(offset_of!(Request, aux_off), 40);
        assert_eq!(offset_of!(Request, aux_len), 48);
    }

    #[test]
    fn reply_layout_matches_the_c_header() {
        assert_eq!(offset_of!(Reply, seq), 0);
        assert_eq!(offset_of!(Reply, compute_ns), 8);
        assert_eq!(offset_of!(Reply, total_ns), 16);
        assert_eq!(offset_of!(Reply, pull_ns), 24);
        assert_eq!(offset_of!(Reply, push_ns), 32);
        assert_eq!(offset_of!(Reply, status), 40);
        assert_eq!(offset_of!(Reply, threads), 44);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn control_words_do_not_share_a_cache_line() {
        assert!(OFF_REQ - OFF_READY >= 64);
        assert!(OFF_REPLY - OFF_REQ >= std::mem::size_of::<Request>());
        assert!(OFF_REPLY - OFF_REQ >= 64);
        assert!(OFF_DATA as usize >= OFF_REPLY + std::mem::size_of::<Reply>());
        assert_eq!(OFF_DATA % BLOCK, 0);
    }

    #[test]
    fn rounding_lands_on_blocks() {
        assert_eq!(round_up(0), 0);
        assert_eq!(round_up(1), 4096);
        assert_eq!(round_up(4096), 4096);
        assert_eq!(round_up(4097), 8192);
    }
}
