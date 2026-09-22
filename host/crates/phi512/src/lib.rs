//! `phi512`: let a program built for AVX-512 run on a machine that has none.
//!
//! The program is not modified, not recompiled, and not aware of this. It
//! executes an AVX-512 instruction, the processor refuses it, and the
//! handler here performs the instruction instead and lets the program
//! carry on.
//!
//! ```text
//! LD_PRELOAD=libphi512.so ./a-program-built-for-avx512
//! ```
//!
//! # The register file is imaginary
//!
//! This host has no `zmm` registers at all, so there is nowhere to keep the
//! values the program believes it is computing with. They live in
//! [`VState`] instead, a plain array. That works only because it is
//! *complete*: the program cannot read a `zmm` register except through an
//! AVX-512 instruction, every one of those faults, and every one of those
//! is serviced here. Nothing else in the process can observe the
//! difference. If a single AVX-512 instruction were executed some other
//! way the illusion would break, which is why anything unrecognised is a
//! hard failure rather than a skip.
//!
//! # Cost
//!
//! A fault costs about 1909 ns on this host, measured, which is roughly
//! 1900 times the instruction it replaces. That is the price of the
//! *first* execution of each site only: because an EVEX instruction is at
//! least 6 bytes and a near jump is 5, a site can be rewritten in place
//! once its translation exists. See `docs/research/avx512-transparency.md`
//! for what each stage costs and where this is going.

use iced_x86::{CpuidFeature, Decoder, DecoderOptions, Instruction, Mnemonic, OpKind, Register};

pub mod emulate;
pub mod frame;
pub mod patch;
pub mod state;

#[cfg(not(test))]
mod handler;

pub use state::VState;

/// Decode one instruction from a byte slice, as the handler does at the
/// faulting address.
pub fn decode_at(bytes: &[u8], ip: u64) -> Instruction {
    Decoder::with_ip(64, bytes, ip, DecoderOptions::NONE).decode()
}

/// Does this instruction need AVX-512, and so fault on a host without it?
///
/// The question is asked of the instruction's *required CPU feature*, not
/// of its encoding, and the difference is not academic. The mask register
/// instructions, `kmovw` and its family, are AVX-512F but are **VEX**
/// encoded, not EVEX. An encoding test misses every one of them, and since
/// masks are the entire point of AVX-512 predication, that is most real
/// AVX-512 programs.
pub fn is_avx512(insn: &Instruction) -> bool {
    insn.cpuid_features().iter().any(|f| {
        matches!(
            f,
            CpuidFeature::AVX512F
                | CpuidFeature::AVX512VL
                | CpuidFeature::AVX512BW
                | CpuidFeature::AVX512DQ
                | CpuidFeature::AVX512CD
                | CpuidFeature::AVX512_VBMI
                | CpuidFeature::AVX512_VNNI
                | CpuidFeature::AVX512_IFMA
                | CpuidFeature::AVX512_BF16
                | CpuidFeature::AVX512_FP16
        )
    })
}

/// The program's scalar state, as the signal frame presents it.
///
/// Reads are needed because memory operands are computed from live
/// register values. Writes are needed because some AVX-512 instructions
/// are not purely vector: `kmov r32, k1` moves a mask into a general
/// register, and `kortest` sets the flags a following `jz` will read.
pub trait Cpu {
    fn get(&self, r: Register) -> u64;
    fn set(&mut self, r: Register, v: u64);
    fn flags(&self) -> u64;
    fn set_flags(&mut self, f: u64);
}

/// Compute the effective address of an instruction's memory operand from
/// the program's live register values.
pub fn effective_address(insn: &Instruction, cpu: &dyn Cpu) -> u64 {
    // RIP-relative is resolved by the decoder, which was given the real
    // instruction pointer, so the answer is already absolute. Adding the
    // next instruction's address on top of it, as the general path below
    // would, produces a wild pointer: this is the form every
    // position-independent load of a constant uses, so getting it wrong
    // crashes almost immediately on real compiler output.
    if insn.is_ip_rel_memory_operand() {
        return insn.ip_rel_memory_address();
    }
    let mut addr = insn.memory_displacement64();
    if insn.memory_base() != Register::None {
        addr = addr.wrapping_add(cpu.get(insn.memory_base()));
    }
    if insn.memory_index() != Register::None {
        let idx = cpu.get(insn.memory_index());
        addr = addr.wrapping_add(idx.wrapping_mul(u64::from(insn.memory_index_scale())));
    }
    addr
}

/// Whether an instruction reads or writes memory at all, which decides
/// whether [`effective_address`] means anything for it.
pub fn touches_memory(insn: &Instruction) -> bool {
    (0..insn.op_count()).any(|i| insn.op_kind(i) == OpKind::Memory)
}

/// A mnemonic this build knows how to perform. Used by the handler to give
/// a precise message instead of a crash when it meets something new.
pub fn is_supported(m: Mnemonic) -> bool {
    emulate::supported(m)
}

/// Diagnostic hook: name an instruction that could not be rewritten
/// because it is shorter than a jump. Wired to the handler's reporting so
/// the cost shows up as a list of mnemonics rather than only a count.
pub fn report_short(rip: u64, len: usize, bytes: &[u8]) {
    if std::env::var_os("PHI512_VERBOSE").is_none() {
        return;
    }
    let insn = decode_at(&bytes[..len.min(15)], rip);
    let msg = format!("phi512: cannot rewrite {:?} ({len} bytes, a jump needs 5)\n", insn.mnemonic());
    // SAFETY: writing a byte buffer we own to stderr.
    unsafe { libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len()) };
}
