//! Translate AVX-512 (EVEX) instructions into Knights Corner (MVEX) ones.
//!
//! The two encodings are close relatives. For the arithmetic that exists on
//! both machines the rewrite touches two places and no others: bit 2 of P1,
//! which EVEX fixes at 1 and MVEX does not use, and the P2 byte, where EVEX
//! packs `z | L'L | b | V' | aaa` and MVEX packs `E | SSS | V' | aaa`. So
//! `vaddps zmm0, zmm1, zmm2` is `62 f1 74 48 58 c2` as AVX-512 and
//! `62 f1 70 08 58 c2` on the card. That is the easy half and it covers
//! most of AVX-512F.
//!
//! The rest of this module is the hard half, and every case in it is a
//! documented difference from `docs/research/avx512-on-knc.md`:
//!
//! - AVX-512 has unaligned 512-bit loads and stores; the card has none, and
//!   each becomes a pair of instructions addressing two cache lines.
//! - AVX-512 has zeroing masking (`{z}`); the card has merging only.
//! - AVX-512 has 128-bit and 256-bit forms; the card is 512-bit only.
//! - `vdivps`, `vsqrtps` and their float64 forms do not exist on the card at
//!   all and have to be synthesised.
//!
//! Anything this module cannot translate is refused by name with a reason,
//! never approximated silently. A wrong answer that looks like a right
//! answer is the one failure mode that would make the whole exercise
//! worthless.

pub mod rewrite;

use iced_x86::{Instruction, Mnemonic, OpKind, Register};
use knc_mvex::{Gpr, Insn, Mem, Src, Zmm, K};

/// An instruction this translator will not translate, and why.
#[derive(Clone, Debug)]
pub struct Unsupported {
    /// The offending instruction in Intel syntax.
    pub text: String,
    /// Why it was refused, in a form fit to print to a user.
    pub reason: String,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.text, self.reason)
    }
}

impl std::error::Error for Unsupported {}

fn refuse(insn: &Instruction, reason: &str) -> Unsupported {
    Unsupported {
        text: format!("{insn}"),
        reason: reason.to_string(),
    }
}

/// Map an iced `zmm` register to the encoder's.
fn zmm(r: Register) -> Option<Zmm> {
    if r.is_zmm() {
        Some(Zmm((r as u32 - Register::ZMM0 as u32) as u8))
    } else {
        None
    }
}

/// Map an iced mask register to the encoder's. `k0` means "no mask", which
/// is how both instruction sets spell an unmasked operation.
fn kreg(r: Register) -> K {
    if r == Register::None || r == Register::K0 {
        K(0)
    } else {
        K((r as u32 - Register::K0 as u32) as u8)
    }
}

/// Map a 64-bit general purpose register to the encoder's.
fn gpr(r: Register) -> Option<Gpr> {
    use Register::*;
    Some(match r {
        RAX => Gpr::Rax,
        RCX => Gpr::Rcx,
        RDX => Gpr::Rdx,
        RBX => Gpr::Rbx,
        RBP => Gpr::Rbp,
        RSI => Gpr::Rsi,
        RDI => Gpr::Rdi,
        R8 => Gpr::R8,
        R9 => Gpr::R9,
        R10 => Gpr::R10,
        R11 => Gpr::R11,
        R13 => Gpr::R13,
        R14 => Gpr::R14,
        R15 => Gpr::R15,
        // rsp and r12 need a SIB byte, which the encoder does not emit.
        _ => return Option::None,
    })
}

/// Pull `[base + disp]` out of an instruction's memory operand, refusing
/// every addressing form the encoder cannot represent.
fn mem(insn: &Instruction) -> Result<Mem, Unsupported> {
    if insn.memory_index() != Register::None {
        return Err(refuse(
            insn,
            "indexed addressing: the MVEX encoder emits no SIB byte, so [base + index*scale] \
             has to be strength-reduced to a pointer increment before translation",
        ));
    }
    if insn.segment_prefix() != Register::None {
        return Err(refuse(insn, "segment prefix"));
    }
    // Refused here, at the site that reports reasons, rather than left to
    // the register map below: `Mem::new` asserts on these two and this
    // tool must never panic on input it can describe.
    if matches!(insn.memory_base(), Register::RSP | Register::R12) {
        return Err(refuse(
            insn,
            "rsp or r12 as a memory base needs a SIB byte, which the MVEX encoder does not emit",
        ));
    }
    let base = insn.memory_base().pipe(gpr).ok_or_else(|| {
        refuse(
            insn,
            "memory base is not a 64-bit register the encoder accepts (rsp and r12 need a SIB byte)",
        )
    })?;
    let disp = insn.memory_displacement64() as i64;
    let disp = i32::try_from(disp).map_err(|_| refuse(insn, "displacement does not fit in 32 bits"))?;
    Ok(Mem::new(base, disp))
}

/// Small helper so `memory_base().pipe(gpr)` reads left to right.
trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

/// The checks that apply to every EVEX instruction before its mnemonic is
/// even considered.
fn check_common(insn: &Instruction) -> Result<(), Unsupported> {
    // AVX-512VL. An EVEX instruction with L'L naming 128 or 256 bits is
    // still EVEX, so it reaches here rather than being caught as legacy
    // AVX, and its mnemonic is in the table below. Translating it would
    // emit a 512-bit MVEX instruction that writes four times the width the
    // program expects. Knights Corner vector instructions are 512-bit only
    // and the machine has no xmm or ymm registers at all (ISA reference
    // appendix B.2), so this is refused explicitly and by name rather than
    // being left to the register-type helpers to reject as a side effect.
    for i in 0..insn.op_count() {
        if insn.op_kind(i) == OpKind::Register {
            let r = insn.op_register(i);
            if r.is_xmm() || r.is_ymm() {
                return Err(refuse(
                    insn,
                    "AVX-512VL: the card is 512-bit only and has no xmm or ymm registers, so a \
                     128-bit or 256-bit operation has to be re-expressed at 512 bits under a \
                     write-mask that limits the active lanes",
                ));
            }
        }
    }
    if insn.zeroing_masking() {
        return Err(refuse(
            insn,
            "zeroing masking {z}: Knights Corner has merging masking only, so this needs the \
             destination cleared first, which changes register pressure and is not a local rewrite",
        ));
    }
    if insn.rounding_control() != iced_x86::RoundingControl::None {
        // MVEX can express this (static rounding, EH=1 plus SSS), but the
        // encoder does not yet take a rounding argument, and guessing here
        // would silently change results.
        return Err(refuse(
            insn,
            "embedded rounding {er}: expressible on the card as MVEX static rounding, not yet \
             wired through the encoder",
        ));
    }
    if insn.is_broadcast() {
        return Err(refuse(
            insn,
            "embedded broadcast {1toN}: expressible on the card as an MVEX swizzle, not yet \
             wired through the encoder",
        ));
    }
    Ok(())
}

/// The second source operand: a register, or memory.
fn src1_and_src2(insn: &Instruction) -> Result<(Zmm, Src), Unsupported> {
    let src1 = zmm(insn.op1_register()).ok_or_else(|| refuse(insn, "first source is not a zmm register"))?;
    let src2 = match insn.op2_kind() {
        OpKind::Register => Src::Reg(zmm(insn.op2_register()).ok_or_else(|| refuse(insn, "second source is not a zmm register"))?),
        OpKind::Memory => Src::Mem(mem(insn)?),
        _ => return Err(refuse(insn, "second source is neither a register nor memory")),
    };
    Ok((src1, src2))
}

/// Translate one AVX-512 instruction into the Knights Corner instructions
/// that reproduce it.
///
/// Returns more than one instruction where the card has no single
/// equivalent; the sequence is always semantically complete, never a
/// truncation.
pub fn translate(insn: &Instruction) -> Result<Vec<Insn>, Unsupported> {
    check_common(insn)?;
    let k = kreg(insn.op_mask());

    match insn.mnemonic() {
        // The mechanical cases: same operation, same lane layout, same
        // IEEE semantics under MXCSR. Only the encoding changes.
        Mnemonic::Vaddps | Mnemonic::Vsubps | Mnemonic::Vmulps | Mnemonic::Vaddpd | Mnemonic::Vsubpd | Mnemonic::Vmulpd => {
            let dst = zmm(insn.op0_register()).ok_or_else(|| refuse(insn, "destination is not a zmm register"))?;
            let (src1, src2) = src1_and_src2(insn)?;
            Ok(vec![match insn.mnemonic() {
                Mnemonic::Vaddps => knc_mvex::vaddps(dst, src1, src2, k),
                Mnemonic::Vsubps => knc_mvex::vsubps(dst, src1, src2, k),
                Mnemonic::Vmulps => knc_mvex::vmulps(dst, src1, src2, k),
                Mnemonic::Vaddpd => knc_mvex::vaddpd(dst, src1, src2, k),
                Mnemonic::Vsubpd => knc_mvex::vsubpd(dst, src1, src2, k),
                _ => knc_mvex::vmulpd(dst, src1, src2, k),
            }])
        }

        // Fused multiply-add. Fused on both machines, single-rounded on
        // both, so this is a re-encode and not an approximation.
        Mnemonic::Vfmadd231ps | Mnemonic::Vfmadd213ps => {
            let dst = zmm(insn.op0_register()).ok_or_else(|| refuse(insn, "destination is not a zmm register"))?;
            let (src1, src2) = src1_and_src2(insn)?;
            Ok(vec![if insn.mnemonic() == Mnemonic::Vfmadd231ps {
                knc_mvex::vfmadd231ps(dst, src1, src2, k)
            } else {
                knc_mvex::vfmadd213ps(dst, src1, src2, k)
            }])
        }
        Mnemonic::Vfmadd231pd => {
            let dst = zmm(insn.op0_register()).ok_or_else(|| refuse(insn, "destination is not a zmm register"))?;
            let (src1, src2) = src1_and_src2(insn)?;
            Ok(vec![knc_mvex::vfmadd231pd(dst, src1, src2, k)])
        }
        Mnemonic::Vfmadd213pd => {
            let dst = zmm(insn.op0_register()).ok_or_else(|| refuse(insn, "destination is not a zmm register"))?;
            let (src1, src2) = src1_and_src2(insn)?;
            Ok(vec![knc_mvex::vfmadd213pd(dst, src1, src2, k)])
        }

        // Integer, 32-bit lanes. The card has no byte or word vector
        // integer instructions at all, so only the `d` forms appear here.
        Mnemonic::Vpaddd | Mnemonic::Vpsubd | Mnemonic::Vpandd | Mnemonic::Vpandnd | Mnemonic::Vpord | Mnemonic::Vpxord => {
            let dst = zmm(insn.op0_register()).ok_or_else(|| refuse(insn, "destination is not a zmm register"))?;
            let (src1, src2) = src1_and_src2(insn)?;
            Ok(vec![match insn.mnemonic() {
                Mnemonic::Vpaddd => knc_mvex::vpaddd(dst, src1, src2, k),
                Mnemonic::Vpsubd => knc_mvex::vpsubd(dst, src1, src2, k),
                Mnemonic::Vpandd => knc_mvex::vpandd(dst, src1, src2, k),
                Mnemonic::Vpandnd => knc_mvex::vpandnd(dst, src1, src2, k),
                Mnemonic::Vpord => knc_mvex::vpord(dst, src1, src2, k),
                _ => knc_mvex::vpxord(dst, src1, src2, k),
            }])
        }

        // Aligned move: one instruction each way, the only 512-bit move the
        // card has that is not a pair.
        Mnemonic::Vmovaps | Mnemonic::Vmovapd | Mnemonic::Vmovdqa32 | Mnemonic::Vmovdqa64 => match (insn.op0_kind(), insn.op1_kind()) {
            (OpKind::Register, OpKind::Memory) => {
                let dst = zmm(insn.op0_register()).ok_or_else(|| refuse(insn, "destination is not a zmm register"))?;
                Ok(vec![knc_mvex::vmovaps_load(dst, mem(insn)?)])
            }
            (OpKind::Memory, OpKind::Register) => {
                let src = zmm(insn.op1_register()).ok_or_else(|| refuse(insn, "source is not a zmm register"))?;
                Ok(vec![knc_mvex::vmovaps_store(mem(insn)?, src)])
            }
            _ => Err(refuse(insn, "register to register moves are not translated yet")),
        },

        // Unaligned move. The card has no `vmovups`, so each one becomes a
        // pair that addresses the two cache lines the access straddles.
        // The pair reads or writes up to 63 bytes beyond the 64 intended,
        // so the buffer has to be padded; that is a property of the card,
        // and the harness that runs translated code must honour it.
        Mnemonic::Vmovups | Mnemonic::Vmovupd | Mnemonic::Vmovdqu32 | Mnemonic::Vmovdqu64 => match (insn.op0_kind(), insn.op1_kind()) {
            (OpKind::Register, OpKind::Memory) => {
                let dst = zmm(insn.op0_register()).ok_or_else(|| refuse(insn, "destination is not a zmm register"))?;
                let lo = mem(insn)?;
                let hi = lo
                    .offset(64)
                    .ok_or_else(|| refuse(insn, "displacement overflows when offset by 64 for the high half"))?;
                Ok(vec![knc_mvex::vloadunpackld(dst, lo, k), knc_mvex::vloadunpackhd(dst, hi, k)])
            }
            (OpKind::Memory, OpKind::Register) => {
                let src = zmm(insn.op1_register()).ok_or_else(|| refuse(insn, "source is not a zmm register"))?;
                let lo = mem(insn)?;
                let hi = lo
                    .offset(64)
                    .ok_or_else(|| refuse(insn, "displacement overflows when offset by 64 for the high half"))?;
                Ok(vec![knc_mvex::vpackstoreld(lo, src, k), knc_mvex::vpackstorehd(hi, src, k)])
            }
            _ => Err(refuse(insn, "register to register moves are not translated yet")),
        },

        // The documented holes, refused by name so that a caller is told
        // exactly which piece of work is missing rather than getting a
        // wrong answer.
        Mnemonic::Vdivps | Mnemonic::Vdivpd | Mnemonic::Vsqrtps | Mnemonic::Vsqrtpd => Err(refuse(
            insn,
            "no divide or square root exists on Knights Corner; this needs the Newton-Raphson \
             expansion described in docs/research/avx512-on-knc.md",
        )),
        Mnemonic::Vmaxps | Mnemonic::Vminps | Mnemonic::Vmaxpd | Mnemonic::Vminpd => Err(refuse(
            insn,
            "the card's vgmax/vgmin use the IEEE-754-2008 NaN rule, not the AVX-512 one; this \
             needs the compare-and-blend expansion",
        )),
        Mnemonic::Vpternlogd | Mnemonic::Vpternlogq => Err(refuse(insn, "vpternlog has no card equivalent; needs truth-table expansion")),
        Mnemonic::Vgatherdps | Mnemonic::Vgatherdpd | Mnemonic::Vpgatherdd | Mnemonic::Vpgatherdq => Err(refuse(
            insn,
            "the card's gather completes only a subset per issue and must be looped until the \
             mask clears, which adds control flow the region model has to carry",
        )),

        _ => Err(refuse(insn, "not in the translation table")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_x86::{Decoder, DecoderOptions};

    /// Decode one instruction and translate it, returning the encoded
    /// bytes of each card instruction produced.
    fn xlate(bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut d = Decoder::with_ip(64, bytes, 0, DecoderOptions::NONE);
        let insn = d.decode();
        translate(&insn).expect("should translate").into_iter().map(|i| i.bytes).collect()
    }

    fn refusal(bytes: &[u8]) -> String {
        let mut d = Decoder::with_ip(64, bytes, 0, DecoderOptions::NONE);
        let insn = d.decode();
        translate(&insn).expect_err("should be refused").reason
    }

    /// For arithmetic that exists on both machines the rewrite touches two
    /// places and no others: bit 2 of P1, which EVEX fixes at 1 and MVEX
    /// does not use (327364-001 section 3.3 gives MVEX.pp two bits), and
    /// the P2 byte, where EVEX packs `z|L'L|b|V'|aaa` and MVEX packs
    /// `E|SSS|V'|aaa`.
    ///
    /// EVEX bytes here are what `llvm-mc -mattr=+avx512f` emits.
    #[test]
    fn vaddps_differs_from_avx512_in_two_places_only() {
        // vaddps zmm0, zmm1, zmm2
        let evex = [0x62, 0xf1, 0x74, 0x48, 0x58, 0xc2];
        let out = xlate(&evex);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], vec![0x62, 0xf1, 0x70, 0x08, 0x58, 0xc2]);
        let differing: Vec<usize> = (0..evex.len()).filter(|&i| evex[i] != out[0][i]).collect();
        assert_eq!(differing, vec![2, 3], "P1 bit 2 and P2, and nothing else");
    }

    /// Bytes verified by execution on the card: this is the instruction at
    /// the centre of `card/examples/avx512_poly.S`, which produces
    /// bit-identical results to the host's FMA3 hardware.
    #[test]
    fn vfmadd231ps_bytes_verified_on_the_card() {
        // vfmadd231ps zmm0, zmm1, zmm2
        let out = xlate(&[0x62, 0xf2, 0x75, 0x48, 0xb8, 0xc2]);
        assert_eq!(out, vec![vec![0x62, 0xf2, 0x71, 0x08, 0xb8, 0xc2]]);
    }

    /// An unaligned load has no single card equivalent and becomes the
    /// pair that names both cache lines the access can straddle. The high
    /// half addresses 64 bytes above the low one.
    ///
    /// The prefix difference between the halves of a load and the halves
    /// of a store is the trap here: loads carry no legacy prefix, stores
    /// carry 66, and both use opcodes D0 and D4.
    #[test]
    fn unaligned_load_becomes_a_pair_addressing_two_cache_lines() {
        // vmovups zmm0, [rdi]
        let out = xlate(&[0x62, 0xf1, 0x7c, 0x48, 0x10, 0x07]);
        assert_eq!(out.len(), 2, "one AVX-512 load, two card instructions");
        assert_eq!(out[0], vec![0x62, 0xf2, 0x78, 0x08, 0xd0, 0x87, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(out[1], vec![0x62, 0xf2, 0x78, 0x08, 0xd4, 0x87, 0x40, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn unaligned_store_becomes_a_pair_and_carries_the_66_prefix() {
        // vmovups [rdi], zmm0
        let out = xlate(&[0x62, 0xf1, 0x7c, 0x48, 0x11, 0x07]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0][2] & 3, 1, "stores carry the 66 prefix");
        assert_eq!(out[0][4], 0xd0);
        assert_eq!(out[1][4], 0xd4);
    }

    /// High registers exercise the encoding extension bits, which are
    /// inverted in MVEX. `card/examples/avx512_poly.S` uses zmm16 to
    /// zmm31 and runs correctly on the card.
    #[test]
    fn high_registers_set_the_inverted_extension_bits() {
        // vaddps zmm31, zmm1, zmm2
        let out = xlate(&[0x62, 0x61, 0x74, 0x48, 0x58, 0xfa]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0][1] >> 7 & 1, 0, "R is inverted and zmm31 sets it");
        assert_eq!(out[0][1] >> 4 & 1, 0, "R' is inverted and zmm31 sets it");
    }

    /// Everything the card cannot do is refused by name. Silence here, or
    /// a plausible substitute, is the one failure that would make the
    /// translator worthless.
    #[test]
    fn the_documented_holes_are_refused_with_a_reason() {
        // vdivps zmm0, zmm1, zmm2
        assert!(refusal(&[0x62, 0xf1, 0x74, 0x48, 0x5e, 0xc2]).contains("divide"));
        // vmaxps zmm0, zmm1, zmm2: the card's NaN rule is not AVX-512's
        assert!(refusal(&[0x62, 0xf1, 0x74, 0x48, 0x5f, 0xc2]).contains("NaN"));
        // vaddps zmm0 {k1}{z}, zmm1, zmm2: no zeroing masking on the card
        assert!(refusal(&[0x62, 0xf1, 0x74, 0xc9, 0x58, 0xc2]).contains("zeroing"));
    }

    /// The one path that could have produced a silently wrong answer.
    ///
    /// An AVX-512VL operation is EVEX-encoded, so it is not caught as
    /// legacy AVX, and its mnemonic is in the translation table. If the
    /// vector length were ignored it would become a 512-bit card
    /// instruction writing four times the width the program asked for.
    ///
    /// EVEX bytes from `llvm-mc -mattr=+avx512f,+avx512vl`; a write mask
    /// is present because without one the assembler picks the shorter VEX
    /// form, which this tool rejects elsewhere.
    #[test]
    fn narrow_evex_forms_are_refused_not_widened() {
        // vaddps xmm0 {k1}, xmm1, xmm2
        assert!(refusal(&[0x62, 0xf1, 0x74, 0x09, 0x58, 0xc2]).contains("AVX-512VL"));
        // vaddps ymm0 {k1}, ymm1, ymm2
        assert!(refusal(&[0x62, 0xf1, 0x74, 0x29, 0x58, 0xc2]).contains("AVX-512VL"));
    }

    /// The encoder asserts on these two bases, so the translator has to
    /// refuse them before it gets there. A panic is not an acceptable
    /// answer for input the tool can name.
    #[test]
    fn sib_only_bases_are_refused_rather_than_panicking() {
        // vmovaps zmm0, [rsp]
        let out = refusal(&[0x62, 0xf1, 0x7c, 0x48, 0x28, 0x04, 0x24]);
        assert!(out.contains("SIB") || out.contains("rsp"), "got: {out}");
    }

    /// A write mask is carried through unchanged, because both machines
    /// put it in the low three bits of the same prefix byte and both mean
    /// merging by default.
    #[test]
    fn write_masks_survive_translation() {
        // vaddps zmm0 {k1}, zmm1, zmm2
        let out = xlate(&[0x62, 0xf1, 0x74, 0x49, 0x58, 0xc2]);
        assert_eq!(out[0][3] & 7, 1, "k1 stays k1");
    }
}
