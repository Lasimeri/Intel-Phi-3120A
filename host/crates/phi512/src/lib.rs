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

use iced_x86::{Decoder, DecoderOptions, Instruction, Mnemonic, OpKind, Register};

pub mod emulate;
pub mod state;

#[cfg(not(test))]
mod handler;

pub use state::VState;

/// Decode one instruction from a byte slice, as the handler does at the
/// faulting address.
pub fn decode_at(bytes: &[u8], ip: u64) -> Instruction {
    Decoder::with_ip(64, bytes, ip, DecoderOptions::NONE).decode()
}

/// Is this an instruction this host cannot execute but we can perform?
pub fn is_avx512(insn: &Instruction) -> bool {
    insn.encoding() == iced_x86::EncodingKind::EVEX
}

/// The program's general purpose registers, as the signal frame presents
/// them. Effective addresses have to be computed from the live values, so
/// the emulator needs to read them, and a few instructions write them.
pub trait Gprs {
    fn get(&self, r: Register) -> u64;
}

/// Compute the effective address of an instruction's memory operand from
/// the program's live register values.
pub fn effective_address(insn: &Instruction, gprs: &dyn Gprs) -> u64 {
    let mut addr = insn.memory_displacement64();
    match insn.memory_base() {
        Register::None => {}
        Register::RIP | Register::EIP => addr = addr.wrapping_add(insn.next_ip()),
        b => addr = addr.wrapping_add(gprs.get(b)),
    }
    if insn.memory_index() != Register::None {
        let idx = gprs.get(insn.memory_index());
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
