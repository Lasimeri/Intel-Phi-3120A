//! The machine state a patched site hands to the emulator.
//!
//! When a fault is serviced, the kernel has already spilled the program's
//! registers into a signal frame and the handler reads them from there.
//! A patched site has no such thing: it runs in the program's own context,
//! with the program's own registers live. So the stub spills them itself,
//! onto its own stack, and passes a pointer to that.
//!
//! Putting the frame on the stack rather than in a thread-local is what
//! makes patched sites thread-safe without any locking: two threads
//! running the same stub have different stacks and therefore different
//! frames, and neither can see the other's.
//!
//! The layout is fixed because the stub is hand-assembled against it. The
//! order below is the order the stub pushes in, read from the lowest
//! address up, so the field order here *is* the instruction order there.

use iced_x86::Register;

use crate::Cpu;

/// Index of each general purpose register in [`PatchFrame::gpr`]. The
/// stub pushes from `r15` down to `rax`, and the stack grows downward, so
/// `rax` ends up lowest.
pub mod gpr_slot {
    pub const RAX: usize = 0;
    pub const RBX: usize = 1;
    pub const RCX: usize = 2;
    pub const RDX: usize = 3;
    pub const RSI: usize = 4;
    pub const RDI: usize = 5;
    pub const RBP: usize = 6;
    pub const R8: usize = 7;
    pub const R9: usize = 8;
    pub const R10: usize = 9;
    pub const R11: usize = 10;
    pub const R12: usize = 11;
    pub const R13: usize = 12;
    pub const R14: usize = 13;
    pub const R15: usize = 14;
    pub const COUNT: usize = 15;
}

/// What a patched stub builds on the stack before calling back.
///
/// `#[repr(C)]` because the stub addresses these fields by byte offset.
/// Changing the order here without changing the stub generator would
/// corrupt every patched site, so both are checked against each other by
/// `layout_matches_the_stub_generator`.
#[repr(C)]
pub struct PatchFrame {
    /// `ymm0` to `ymm15` as the program left them. These are the real
    /// registers: on this host they *are* the low 256 bits of what the
    /// program believes are `zmm` registers, so they are the live values,
    /// not a copy of them.
    pub ymm: [[u8; 32]; 16],
    /// `rflags`, saved before anything in the stub could disturb them.
    pub flags: u64,
    /// The general purpose registers, indexed by [`gpr_slot`].
    pub gpr: [u64; gpr_slot::COUNT],
}

impl PatchFrame {
    /// Byte offset of the ymm save area, where the stub writes it.
    pub const YMM_OFFSET: usize = 0;
    /// Byte offset of the saved flags.
    pub const FLAGS_OFFSET: usize = 16 * 32;
    /// Byte offset of the first general purpose register.
    pub const GPR_OFFSET: usize = Self::FLAGS_OFFSET + 8;
    /// Total size, and the distance from the frame to the return address
    /// the trampoline was called with.
    pub const SIZE: usize = Self::GPR_OFFSET + gpr_slot::COUNT * 8;

    /// How far the program's original `rsp` is above the frame.
    ///
    /// The stub steps over the 128-byte red zone, calls (pushing eight
    /// bytes of return address), and the trampoline then lays down this
    /// frame. So counting back up from the frame: the frame itself, the
    /// return address, the red zone.
    pub const RSP_ABOVE_FRAME: usize = Self::SIZE + 8 + 128;

    fn slot(r: Register) -> Option<usize> {
        use gpr_slot::*;
        Some(match r.full_register() {
            Register::RAX => RAX,
            Register::RBX => RBX,
            Register::RCX => RCX,
            Register::RDX => RDX,
            Register::RSI => RSI,
            Register::RDI => RDI,
            Register::RBP => RBP,
            Register::R8 => R8,
            Register::R9 => R9,
            Register::R10 => R10,
            Register::R11 => R11,
            Register::R12 => R12,
            Register::R13 => R13,
            Register::R14 => R14,
            Register::R15 => R15,
            _ => return None,
        })
    }
}

/// A [`PatchFrame`] plus the one register that is not in it.
///
/// `rsp` cannot be pushed like the others: by the time the trampoline
/// could push it, it has already changed. It is reconstructed instead,
/// from the frame's own address, because the distance between them is
/// fixed by the stub and trampoline that built it.
pub struct PatchCpu<'a> {
    pub frame: &'a mut PatchFrame,
    rsp: u64,
}

impl<'a> PatchCpu<'a> {
    /// # Safety
    /// `frame` must point at a frame laid down by the trampoline, so that
    /// the program's stack really is `RSP_ABOVE_FRAME` bytes above it.
    pub unsafe fn new(frame: &'a mut PatchFrame) -> PatchCpu<'a> {
        let rsp = frame as *const PatchFrame as u64 + PatchFrame::RSP_ABOVE_FRAME as u64;
        PatchCpu { frame, rsp }
    }
}

impl Cpu for PatchCpu<'_> {
    fn get(&self, r: Register) -> u64 {
        if r.full_register() == Register::RSP {
            return self.rsp;
        }
        match PatchFrame::slot(r) {
            Some(i) => self.frame.gpr[i],
            None => 0,
        }
    }

    fn set(&mut self, r: Register, v: u64) {
        // Writing a 32-bit register zeroes the upper half, the x86-64
        // rule, which matters for `kmov eax, k1`.
        let v = if r.size() == 4 { v & 0xffff_ffff } else { v };
        if let Some(i) = PatchFrame::slot(r) {
            self.frame.gpr[i] = v;
        }
    }

    fn flags(&self) -> u64 {
        self.frame.flags
    }

    fn set_flags(&mut self, f: u64) {
        self.frame.flags = f;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stub generator hard-codes these offsets. If the structure
    /// changes and the generator does not, every patched site writes the
    /// program's registers into the wrong places, which would be a very
    /// confusing way to find out.
    #[test]
    fn layout_matches_the_stub_generator() {
        assert_eq!(PatchFrame::YMM_OFFSET, 0);
        assert_eq!(PatchFrame::FLAGS_OFFSET, 512);
        assert_eq!(PatchFrame::GPR_OFFSET, 520);
        assert_eq!(PatchFrame::SIZE, 640);
        assert_eq!(PatchFrame::RSP_ABOVE_FRAME, 776);

        assert_eq!(std::mem::size_of::<PatchFrame>(), PatchFrame::SIZE);
    }

    #[test]
    fn the_stack_pointer_reads_back_from_its_own_slot() {
        let mut inner = PatchFrame {
            ymm: [[0; 32]; 16],
            flags: 0,
            gpr: [0; gpr_slot::COUNT],
        };
        // SAFETY: a synthetic frame; only the arithmetic is under test.
        let mut f = unsafe { PatchCpu::new(&mut inner) };
        let want = (f.frame as *const PatchFrame as u64) + PatchFrame::RSP_ABOVE_FRAME as u64;
        assert_eq!(f.get(Register::RSP), want, "rsp is reconstructed from the frame address");
        f.set(Register::RAX, 0x1234_5678_9abc_def0);
        assert_eq!(f.get(Register::RAX), 0x1234_5678_9abc_def0);
        // eax is the same register, and writing it clears the top half
        f.set(Register::EAX, 0xffff_ffff_ffff_ffff);
        assert_eq!(f.get(Register::RAX), 0xffff_ffff);
    }
}
