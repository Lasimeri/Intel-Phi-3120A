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
    /// The low 32 bytes of zmm0 to zmm15 as this library last left
    /// them. Used to notice that something else wrote the register.
    /// See `note_write` and `upper_is_stale`.
    pub last_low: [[u8; 32]; 16],
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
            last_low: [[0u8; 32]; 16],
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

impl VState {
    /// Record the low 256 bits of every aliasable register as this
    /// library is leaving them.
    pub fn note_write(&mut self) {
        for (last, reg) in self.last_low.iter_mut().zip(&self.zmm) {
            last.copy_from_slice(&reg[..32]);
        }
    }

    /// Has something other than this library written register `i` since
    /// the last emulated instruction?
    ///
    /// This matters because of a rule that has no visible effect on a
    /// machine without AVX-512, and a decisive one on a machine
    /// pretending to have it: **a VEX-encoded write to `xmm` or `ymm`
    /// zeroes the whole 512-bit register**. On real hardware
    /// `vpxor xmm0, xmm0, xmm0` clears bits 0 to 511. Here it clears bits
    /// 0 to 255, because that is all the silicon there is, and bits 256
    /// to 511 are this library's storage, which it never hears about.
    ///
    /// VEX instructions do not fault, so they cannot be observed. What
    /// can be observed is their effect: if the low 256 bits in the signal
    /// frame differ from what was left there, something else wrote the
    /// register, and on real hardware that write would have zeroed the
    /// top.
    ///
    /// The inference is not perfect. A legacy SSE write preserves the
    /// upper bits rather than zeroing them, so it would be treated too
    /// harshly; and a write that happened to reproduce the previous 256
    /// bits exactly would be missed. Compilers targeting AVX-512 emit VEX
    /// and EVEX, not legacy SSE, and the second case requires a
    /// coincidence in 256 bits.
    pub fn upper_is_stale(&self, i: usize, live_low: &[u8]) -> bool {
        i < 16 && live_low != self.last_low[i]
    }
}

/// The imaginary registers, one set per thread.
///
/// Both ways into the emulator use this same storage: the fault handler,
/// and a patched site calling back. A program that has some of its
/// AVX-512 sites patched and some not must still see one coherent set of
/// registers, so there is exactly one of these per thread and not one per
/// entry path.
///
/// `const` initialisation matters: the first touch may be inside a signal
/// handler, where allocating would be a deadlock waiting to happen.
pub mod tls {
    use super::VState;
    use std::cell::UnsafeCell;

    thread_local! {
        static STATE: UnsafeCell<VState> = const { UnsafeCell::new(VState::new()) };
    }

    /// Run `f` with this thread's registers.
    ///
    /// The `&mut` is sound because the state is thread-local and neither
    /// caller re-enters: a signal handler cannot interrupt itself, and a
    /// patched site cannot fault while inside the emulator.
    pub fn with<R>(f: impl FnOnce(&mut VState) -> R) -> R {
        STATE.with(|s| f(unsafe { &mut *s.get() }))
    }
}

impl VState {
    /// Take the live low 256 bits of `zmm0` to `zmm15` from wherever the
    /// caller found them, applying the VEX-zeroing rule on the way in.
    ///
    /// Both entry paths need this and both get the values from somewhere
    /// different: the fault handler reads them out of the signal frame's
    /// XSAVE area, a patched site reads them off its own stack. The rule
    /// applied to them is the same, so it lives here rather than in
    /// either one.
    pub fn sync_in(&mut self, live: &[[u8; 32]; 16]) {
        self.sync_in_masked(live, u32::MAX);
    }

    /// Write the low 256 bits back and remember them, so the next
    /// `sync_in` can tell whether anything else has been at them.
    pub fn sync_out(&mut self, live: &mut [[u8; 32]; 16]) {
        self.sync_out_masked(live, u32::MAX);
    }
}

impl VState {
    /// The same as [`VState::sync_in`], but only for the registers named
    /// by `mask`.
    ///
    /// An instruction names at most three or four vector registers, so
    /// synchronising all sixteen copies about a kilobyte per execution to
    /// no purpose. The set is worked out once, when the site is rewritten.
    ///
    /// Skipping a register is safe for the staleness rule as well as for
    /// the value: `last_low` is only updated for registers that were
    /// synchronised, so a register left alone still carries whatever it
    /// was last seen holding, and the comparison that detects an outside
    /// write is still valid the next time it is looked at.
    pub fn sync_in_masked(&mut self, live: &[[u8; 32]; 16], mask: u32) {
        for (i, live) in live.iter().enumerate() {
            if mask & (1 << i) == 0 {
                continue;
            }
            if self.upper_is_stale(i, live) {
                self.zmm[i][32..].fill(0);
            }
            self.zmm[i][..32].copy_from_slice(live);
        }
    }

    /// The counterpart of [`VState::sync_in_masked`].
    pub fn sync_out_masked(&mut self, live: &mut [[u8; 32]; 16], mask: u32) {
        for (i, live) in live.iter_mut().enumerate() {
            if mask & (1 << i) == 0 {
                continue;
            }
            live.copy_from_slice(&self.zmm[i][..32]);
            self.last_low[i].copy_from_slice(&self.zmm[i][..32]);
        }
    }
}
