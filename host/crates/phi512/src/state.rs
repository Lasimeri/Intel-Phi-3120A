//! The imaginary register file.

use std::fmt;

/// The AVX-512 architectural state this host does not have: 32 vector
/// registers of 512 bits, and 8 mask registers.
///
/// Held as bytes rather than as typed lanes because the same 64 bytes are
/// read as float32, float64, int32 or int64 depending on the instruction,
/// and reinterpreting bytes is exactly what the hardware does.
#[repr(C, align(64))]
#[derive(Clone)]
pub struct VState {
    pub zmm: [[u8; 64]; 32],
    pub k: [u64; 8],
}

impl Default for VState {
    fn default() -> Self {
        Self::new()
    }
}

impl VState {
    /// All registers zero, which is the state `xsave` would report after
    /// the kernel first gives a thread its vector state.
    pub const fn new() -> VState {
        VState {
            zmm: [[0u8; 64]; 32],
            k: [0u64; 8],
        }
    }

    /// Read one 32-bit lane of a register as float32.
    pub fn f32_lane(&self, reg: usize, lane: usize) -> f32 {
        let o = lane * 4;
        f32::from_le_bytes(self.zmm[reg][o..o + 4].try_into().unwrap())
    }

    pub fn set_f32_lane(&mut self, reg: usize, lane: usize, v: f32) {
        let o = lane * 4;
        self.zmm[reg][o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn f64_lane(&self, reg: usize, lane: usize) -> f64 {
        let o = lane * 8;
        f64::from_le_bytes(self.zmm[reg][o..o + 8].try_into().unwrap())
    }

    pub fn set_f64_lane(&mut self, reg: usize, lane: usize, v: f64) {
        let o = lane * 8;
        self.zmm[reg][o..o + 8].copy_from_slice(&v.to_le_bytes());
    }

    pub fn i32_lane(&self, reg: usize, lane: usize) -> i32 {
        let o = lane * 4;
        i32::from_le_bytes(self.zmm[reg][o..o + 4].try_into().unwrap())
    }

    pub fn set_i32_lane(&mut self, reg: usize, lane: usize, v: i32) {
        let o = lane * 4;
        self.zmm[reg][o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Is lane `lane` enabled by mask register `k`? `k0` means no mask,
    /// which enables everything: that is the architectural meaning of
    /// encoding zero in the mask field, not a special case invented here.
    pub fn lane_enabled(&self, k: usize, lane: usize) -> bool {
        k == 0 || (self.k[k] >> lane) & 1 == 1
    }
}

impl fmt::Debug for VState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VState {{ zmm0: {:02x?}.., k1: {:#x} }}", &self.zmm[0][..8], self.k[1])
    }
}
