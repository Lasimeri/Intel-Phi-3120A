//! Byte-level EVEX to MVEX rewriting for the seamless path.
//!
//! The card's MVEX prefix and AVX-512's EVEX prefix share the four-byte
//! shape, the opcode maps, and the ModRM, SIB and displacement bytes
//! that follow (including the compressed disp8*N scaling), so for the
//! instructions the card has one to one the rewrite is the prefix
//! payload alone and the instruction keeps its length: P1 bit 2 clears,
//! and P2's `z L'L b V' aaa` becomes `E SSS V' aaa`, with SSS 001 when
//! EVEX had a memory broadcast ({1to16}, {1to8}) and 000 otherwise. That
//! is what `rewrite` does for the whitelist below, regardless of how the
//! memory operand is addressed: indexed, scaled, RIP-relative, all copied
//! as they are.
//!
//! A few instructions the card lacks are expressed as a sequence placed
//! out of line: the site becomes `jmp rel32` into a thunk area that runs
//! the sequence and jumps back. `Rewrite::Thunk` carries the sequence;
//! the caller lays it out (`thunk_bytes`) because only it knows the
//! addresses. Everything else is refused with the reason, and the
//! caller decides what a refusal means for the region.
//!
//! Encodings are from the ISA reference 327364-001 (opcode lines quoted
//! on each table row) and were checked on the card by running the
//! program that uses them (`tools/avx512-seamless-test.c`, results in
//! `docs/results/2026-09-22-seamless-card.md`).

use iced_x86::{EncodingKind, Instruction, Mnemonic, OpKind, Register};
use knc_mvex::{Src, Zmm, K};

use crate::Unsupported;

/// What a site becomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rewrite {
    /// The same length, in place.
    InPlace(Vec<u8>),
    /// Out of line: the site becomes `jmp` to `seq` followed by `jmp` back.
    Thunk(Vec<u8>),
}

fn refuse(insn: &Instruction, reason: &str) -> Unsupported {
    Unsupported {
        text: format!("{insn}"),
        reason: reason.to_string(),
    }
}

/// zmm number of a zmm, ymm or xmm register operand.
fn vreg(r: Register) -> Option<u8> {
    if r.is_zmm() {
        Some((r as u32 - Register::ZMM0 as u32) as u8)
    } else if r.is_ymm() {
        Some((r as u32 - Register::YMM0 as u32) as u8)
    } else if r.is_xmm() {
        Some((r as u32 - Register::XMM0 as u32) as u8)
    } else {
        None
    }
}

/// The one-to-one whitelist: (map, pp, W, opcode), plus the ModRM.reg
/// field for the shift group. Each row names the card instruction whose
/// opcode line in 327364-001 it was checked against.
fn one_to_one(map: u8, pp: u8, w: u8, op: u8, reg: u8) -> bool {
    match (map, pp, w, op) {
        // vmovaps / vmovapd load, store, register (MVEX.512.0F.W0 28, 29; .66.W1 for pd)
        (1, 0, 0, 0x28) | (1, 0, 0, 0x29) | (1, 1, 1, 0x28) | (1, 1, 1, 0x29) => true,
        // vaddps vmulps vsubps (MVEX.NDS.512.0F.W0 58, 59, 5C) and the pd forms
        (1, 0, 0, 0x58) | (1, 0, 0, 0x59) | (1, 0, 0, 0x5C) => true,
        (1, 1, 1, 0x58) | (1, 1, 1, 0x59) | (1, 1, 1, 0x5C) => true,
        // vmovdqa32 / vmovdqa64 (MVEX.512.66.0F.W0 6F, 7F; W1)
        (1, 1, 0, 0x6F) | (1, 1, 0, 0x7F) | (1, 1, 1, 0x6F) | (1, 1, 1, 0x7F) => true,
        // vpaddd vpsubd vpandd vpandnd vpord vpxord (MVEX.NDS.512.66.0F.W0 FE FA DB DF EB EF)
        (1, 1, 0, 0xFE) | (1, 1, 0, 0xFA) | (1, 1, 0, 0xDB) | (1, 1, 0, 0xDF) | (1, 1, 0, 0xEB) | (1, 1, 0, 0xEF) => true,
        // vpandq vpandnq vporq vpxorq (W1)
        (1, 1, 1, 0xDB) | (1, 1, 1, 0xDF) | (1, 1, 1, 0xEB) | (1, 1, 1, 0xEF) => true,
        // vpcmpeqd vpcmpgtd into a mask (MVEX.NDS.512.66.0F.W0 76, 66)
        (1, 1, 0, 0x76) | (1, 1, 0, 0x66) => true,
        // vpsrld /2, vpsrad /4, vpslld /6 by immediate (MVEX.NDD.512.66.0F.W0 72)
        (1, 1, 0, 0x72) => matches!(reg, 2 | 4 | 6),
        // vpshufd imm (MVEX.512.66.0F.W0 70)
        (1, 1, 0, 0x70) => true,
        // vpcmpd / vpcmpud with a predicate into a mask (MVEX.NDS.512.66.0F3A.W0 1F, 1E ib): the
        // eight predicates are numbered the same (EQ LT LE FALSE NE NLT NLE TRUE)
        (3, 1, 0, 0x1F) | (3, 1, 0, 0x1E) => true,
        // vcmpps / vcmppd (MVEX.NDS.512.0F.W0 C2 ib; .66.W1): predicates 0 to 7 only, checked below
        (1, 0, 0, 0xC2) | (1, 1, 1, 0xC2) => true,
        // vbroadcastss / vbroadcastsd from memory (MVEX.512.66.0F38.W0 18; W1 19)
        (2, 1, 0, 0x18) | (2, 1, 1, 0x19) => true,
        // vfmadd, vfmsub, vfnmadd, vfnmsub 132/213/231 ps (W0) and pd (W1): 98 A8 B8, 9A AA BA, 9C AC BC, 9E AE BE
        (2, 1, _, 0x98)
        | (2, 1, _, 0xA8)
        | (2, 1, _, 0xB8)
        | (2, 1, _, 0x9A)
        | (2, 1, _, 0xAA)
        | (2, 1, _, 0xBA)
        | (2, 1, _, 0x9C)
        | (2, 1, _, 0xAC)
        | (2, 1, _, 0xBC)
        | (2, 1, _, 0x9E)
        | (2, 1, _, 0xAE)
        | (2, 1, _, 0xBE) => true,
        // vpmulld, vpermd, vpsrlvd, vpsllvd, vpminsd, vpminud, vpmaxsd, vpmaxud (MVEX.NDS.512.66.0F38.W0 40 36 45 47 39 3B 3D 3F)
        (2, 1, 0, 0x40)
        | (2, 1, 0, 0x36)
        | (2, 1, 0, 0x45)
        | (2, 1, 0, 0x47)
        | (2, 1, 0, 0x39)
        | (2, 1, 0, 0x3B)
        | (2, 1, 0, 0x3D)
        | (2, 1, 0, 0x3F) => true,
        _ => false,
    }
}

/// `kmov eax, kN` and `kmov kN, eax` (VEX.128.0F.W0 93 /r and 92 /r,
/// the same bytes on the card and in AVX-512).
fn kmov_to_eax(k: u8) -> [u8; 4] {
    [0xc5, 0xf8, 0x93, 0xc0 | k]
}
fn kmov_from_eax(k: u8) -> [u8; 4] {
    [0xc5, 0xf8, 0x92, 0xc0 | (k << 3)]
}

/// `vpermf32x4 zmm_d, zmm_s, imm` (MVEX.512.66.0F3A.W0 07 /r ib): 128-bit
/// blocks of the source placed by the immediate.
fn vpermf32x4(d: u8, s: u8, imm: u8) -> [u8; 7] {
    let p0 = ((!(d >> 3) & 1) << 7) | ((!(s >> 4) & 1) << 6) | ((!(s >> 3) & 1) << 5) | ((!(d >> 4) & 1) << 4) | 3;
    [0x62, p0, 0x79, 0x08, 0x07, 0xc0 | ((d & 7) << 3) | (s & 7), imm]
}

/// `vmovaps zmm_d, zmm_s` (MVEX.512.0F.W0 28 /r).
fn vmovaps_reg(d: u8, s: u8) -> [u8; 6] {
    let p0 = ((!(d >> 3) & 1) << 7) | ((!(s >> 4) & 1) << 6) | ((!(s >> 3) & 1) << 5) | ((!(d >> 4) & 1) << 4) | 1;
    [0x62, p0, 0x78, 0x08, 0x28, 0xc0 | ((d & 7) << 3) | (s & 7)]
}

/// The sequence that zeroes the lanes of `d` selected by `mask`, keeping
/// k7 and rax as they were. Uses the program's stack for two words.
fn zero_lanes(d: u8, mask: u16) -> Vec<u8> {
    let mut v = vec![0x50]; // push rax
    v.extend_from_slice(&kmov_to_eax(7));
    v.push(0x50); // push rax
    v.push(0xb8); // mov eax, imm32
    v.extend_from_slice(&u32::from(mask).to_le_bytes());
    v.extend_from_slice(&kmov_from_eax(7));
    v.extend_from_slice(&knc_mvex::vpxord(Zmm(d), Zmm(d), Src::Reg(Zmm(d)), K(7)).bytes);
    v.push(0x58); // pop rax
    v.extend_from_slice(&kmov_from_eax(7));
    v.push(0x58); // pop rax
    v
}

/// Rewrite one AVX-512 instruction for the card.
pub fn rewrite(insn: &Instruction, bytes: &[u8]) -> Result<Rewrite, Unsupported> {
    if insn.encoding() != EncodingKind::EVEX || bytes.len() < 6 || bytes[0] != 0x62 {
        return Err(refuse(insn, "not an EVEX instruction"));
    }
    let (p0, p1, p2) = (bytes[1], bytes[2], bytes[3]);
    let map = p0 & 3;
    let pp = p1 & 3;
    let w = p1 >> 7;
    let z = p2 >> 7;
    let ll = (p2 >> 5) & 3;
    let b = (p2 >> 4) & 1;
    let op = bytes[4];
    let modrm = bytes[5];
    let reg = (modrm >> 3) & 7;
    let has_mem = (0..insn.op_count()).any(|i| insn.op_kind(i) == OpKind::Memory);

    // The few sequences first: they have their own operand shapes.
    match insn.mnemonic() {
        Mnemonic::Vextractf64x4 | Mnemonic::Vextractf32x4 | Mnemonic::Vextracti64x4 | Mnemonic::Vextracti32x4 => {
            if insn.op0_kind() != OpKind::Register || insn.op_mask() != Register::None || z == 1 {
                return Err(refuse(insn, "extract to memory or under a mask: not expressed on the card yet"));
            }
            let d = vreg(insn.op0_register()).ok_or_else(|| refuse(insn, "extract destination is not a vector register"))?;
            let s = vreg(insn.op1_register()).ok_or_else(|| refuse(insn, "extract source is not a zmm register"))?;
            let imm = insn.immediate8();
            let wide = matches!(insn.mnemonic(), Mnemonic::Vextractf64x4 | Mnemonic::Vextracti64x4);
            let mut seq = Vec::new();
            // Bring the selected 128-bit block(s) to the bottom, then zero what a ymm or xmm write leaves zero.
            if wide {
                if imm & 1 == 1 {
                    seq.extend_from_slice(&vpermf32x4(d, s, 0x4e));
                } else if d != s {
                    seq.extend_from_slice(&vmovaps_reg(d, s));
                }
                seq.extend(zero_lanes(d, 0xff00));
            } else {
                let block = imm & 3;
                if block != 0 {
                    seq.extend_from_slice(&vpermf32x4(d, s, block));
                } else if d != s {
                    seq.extend_from_slice(&vmovaps_reg(d, s));
                }
                seq.extend(zero_lanes(d, 0xfff0));
            }
            return Ok(Rewrite::Thunk(seq));
        }
        Mnemonic::Vpternlogd | Mnemonic::Vpternlogq => {
            // Only the idioms compilers emit: all ones (0xff) and zero (0x00).
            let d = vreg(insn.op0_register()).ok_or_else(|| refuse(insn, "vpternlog destination is not a register"))?;
            if insn.op_mask() != Register::None || z == 1 {
                return Err(refuse(insn, "vpternlog under a mask"));
            }
            return match insn.immediate8() {
                0xff => Ok(Rewrite::Thunk(all_ones(d))),
                0x00 => Ok(Rewrite::Thunk(knc_mvex::vpxord(Zmm(d), Zmm(d), Src::Reg(Zmm(d)), K(0)).bytes)),
                _ => Err(refuse(insn, "vpternlog with a truth table other than all ones or zero")),
            };
        }
        Mnemonic::Vmovups | Mnemonic::Vmovupd | Mnemonic::Vmovdqu32 | Mnemonic::Vmovdqu64 => {
            if z == 1 || ll != 2 {
                return Err(refuse(insn, "unaligned move: zeroing or not 512-bit"));
            }
            if !has_mem {
                // Register to register: the aligned move does the same thing.
                let mut v = bytes[..insn.len()].to_vec();
                v[1] = (v[1] & 0xfc) | 1;
                v[2] = (v[2] & !0x04 & !0x03) | if w == 1 { 1 } else { 0 };
                v[3] = p2 & 0x0f;
                v[4] = if insn.op0_kind() == OpKind::Register { 0x28 } else { 0x29 };
                return Ok(Rewrite::InPlace(v));
            }
            return unaligned(insn, bytes, w, p2 & 0x0f);
        }
        _ => {}
    }

    if z == 1 {
        return Err(refuse(insn, "zeroing masking {z}: the card merges only"));
    }
    if ll != 2 {
        return Err(refuse(insn, "a 128-bit or 256-bit form: the card is 512-bit only"));
    }
    for i in 0..insn.op_count() {
        if insn.op_kind(i) == OpKind::Register {
            let r = insn.op_register(i);
            if r.is_xmm() || r.is_ymm() {
                return Err(refuse(insn, "an xmm or ymm operand: the card is 512-bit only"));
            }
        }
    }
    if b == 1 && !has_mem {
        return Err(refuse(insn, "embedded rounding or SAE: not expressed on the card yet"));
    }
    if !one_to_one(map, pp, w, op, reg) {
        return Err(refuse(insn, "not in the card's one-to-one table"));
    }
    if map == 1 && op == 0xC2 && insn.immediate8() > 7 {
        return Err(refuse(
            insn,
            "a float compare predicate above 7: the card has the eight classic ones",
        ));
    }
    let mut v = bytes[..insn.len()].to_vec();
    v[2] = p1 & !0x04;
    v[3] = (if b == 1 { 1 << 4 } else { 0 }) | (p2 & 0x0f);
    Ok(Rewrite::InPlace(v))
}

/// All ones into `d`: `vbroadcastss d, [rip + ones]` with the constant
/// placed right after the jump back, which the layout (`thunk_bytes`)
/// knows to append. The sequence is marked with the placeholder
/// displacement `ALL_ONES_DISP`; `thunk_bytes` patches it.
pub const ALL_ONES_DISP: u32 = 0x5a5a_5a5a;
fn all_ones(d: u8) -> Vec<u8> {
    let p0 = ((!(d >> 3) & 1) << 7) | (1 << 6) | (1 << 5) | ((!(d >> 4) & 1) << 4) | 2;
    let mut v = vec![0x62, p0, 0x79, 0x08, 0x18, 0x05 | ((d & 7) << 3)];
    v.extend_from_slice(&ALL_ONES_DISP.to_le_bytes());
    v
}

/// An unaligned load or store as the card's unpack/pack pair
/// (vloadunpackld/hd MVEX.512.0F38.W0 D0/D4, vpackstoreld/hd
/// MVEX.512.66.0F38.W0 D0/D4; the q forms D1/D5), each with the original
/// memory operand, the high half at +64.
fn unaligned(insn: &Instruction, bytes: &[u8], w: u8, vaaa: u8) -> Result<Rewrite, Unsupported> {
    let load = insn.op0_kind() == OpKind::Register;
    let modrm = bytes[5];
    let mo = modrm >> 6;
    let rm = modrm & 7;
    let tail = &bytes[6..insn.len()]; // SIB and displacement as encoded
    let (sib, disp): (Option<u8>, &[u8]) = if rm == 4 { (Some(tail[0]), &tail[1..]) } else { (None, tail) };
    let p0 = bytes[1];
    let p1 = 0x78 | if load { 0 } else { 1 }; // W0, no vvvv, pp 00 or 66
    let opc = |hi: bool| -> u8 { (if hi { 0xd4 } else { 0xd0 }) | w };
    // Second operand: the same address plus 64, which is one disp8 unit here (N = 64).
    let (modrm2, disp2): (u8, Vec<u8>) = match mo {
        0 if rm == 5 || (rm == 4 && sib.is_some_and(|s| s & 7 == 5)) => {
            // disp32 form (RIP-relative, or SIB with no base)
            let d = i32::from_le_bytes(disp[..4].try_into().unwrap()).checked_add(64);
            (
                modrm,
                d.ok_or_else(|| refuse(insn, "displacement overflows"))?.to_le_bytes().to_vec(),
            )
        }
        0 => ((1 << 6) | (modrm & 0x3f), vec![1]),
        1 => {
            let d = disp[0] as i8;
            if d == i8::MAX {
                // Becomes a disp32 of (d*64 + 64).
                ((2 << 6) | (modrm & 0x3f), ((i32::from(d) * 64) + 64).to_le_bytes().to_vec())
            } else {
                (modrm, vec![(d + 1) as u8])
            }
        }
        2 => {
            let d = i32::from_le_bytes(disp[..4].try_into().unwrap()).checked_add(64);
            (
                modrm,
                d.ok_or_else(|| refuse(insn, "displacement overflows"))?.to_le_bytes().to_vec(),
            )
        }
        _ => return Err(refuse(insn, "unaligned move with a register operand")),
    };
    let mut seq = vec![0x62, (p0 & 0xfc) | 2, p1, vaaa, opc(false), modrm];
    if let Some(s) = sib {
        seq.push(s);
    }
    seq.extend_from_slice(disp);
    seq.extend_from_slice(&[0x62, (p0 & 0xfc) | 2, p1, vaaa, opc(true), modrm2]);
    if let Some(s) = sib {
        seq.push(s);
    }
    seq.extend_from_slice(&disp2);
    Ok(Rewrite::Thunk(seq))
}

/// Lay a thunk out at `at` for a site at `site` of `len` bytes whose
/// next instruction is `next`: the sequence, `jmp next`, and for a
/// sequence with the all-ones placeholder the constant it addresses.
/// Returns the thunk bytes and the site's replacement (`jmp at` padded
/// with NOPs to `len`).
pub fn thunk_bytes(seq: &[u8], at: u64, site: u64, len: usize, next: u64) -> (Vec<u8>, Vec<u8>) {
    let mut t = seq.to_vec();
    let back = next as i64 - (at as i64 + t.len() as i64 + 5);
    t.push(0xe9);
    t.extend_from_slice(&(back as i32).to_le_bytes());
    // Patch a placeholder to point at a constant appended here.
    let marker = ALL_ONES_DISP.to_le_bytes();
    if let Some(pos) = seq.windows(4).position(|w| w == marker) {
        // The card faults on a misaligned vector memory operand: the
        // constant goes on a 64-byte boundary.
        while (at + t.len() as u64) % 64 != 0 {
            t.push(0xcc);
        }
        let const_at = at + t.len() as u64;
        let insn_end = at + pos as u64 + 4;
        let rel = (const_at as i64 - insn_end as i64) as i32;
        t[pos..pos + 4].copy_from_slice(&rel.to_le_bytes());
        t.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    }
    let mut s = vec![0xe9];
    s.extend_from_slice(&((at as i64 - (site as i64 + 5)) as i32).to_le_bytes());
    while s.len() < len {
        s.push(0x90);
    }
    (t, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_x86::{Decoder, DecoderOptions};

    fn dec(bytes: &[u8]) -> Instruction {
        Decoder::with_ip(64, bytes, 0x1000, DecoderOptions::NONE).decode()
    }

    #[test]
    fn indexed_fmadd_with_broadcast_keeps_its_sib_and_gets_the_swizzle() {
        // vfmadd213ps (%r9,%rax,4){1to16},%zmm1,%zmm0 as gcc emitted it
        let b = [0x62, 0xd2, 0x75, 0x58, 0xa8, 0x04, 0x81];
        match rewrite(&dec(&b), &b).unwrap() {
            Rewrite::InPlace(v) => {
                assert_eq!(v.len(), b.len());
                assert_eq!(&v[4..], &b[4..], "opcode, ModRM and SIB untouched");
                assert_eq!(v[2], 0x71, "P1 bit 2 cleared");
                assert_eq!(v[3], 0x18, "SSS = 001 for {{1to16}}, V' and aaa kept");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn aligned_load_with_sib_is_one_to_one() {
        // vmovaps (%r14,%rdx,4),%zmm1
        let b = [0x62, 0xc1, 0x7c, 0x48, 0x28, 0x0c, 0x96];
        match rewrite(&dec(&b), &b).unwrap() {
            Rewrite::InPlace(v) => assert_eq!(v, vec![0x62, 0xc1, 0x78, 0x08, 0x28, 0x0c, 0x96]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn masked_mullo_keeps_its_mask() {
        // vpmulld %zmm3,%zmm1,%zmm0{%k1}
        let b = [0x62, 0xf2, 0x75, 0x49, 0x40, 0xc3];
        match rewrite(&dec(&b), &b).unwrap() {
            Rewrite::InPlace(v) => assert_eq!(v, vec![0x62, 0xf2, 0x71, 0x09, 0x40, 0xc3]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn extract_high_half_is_a_permute_and_a_masked_zero() {
        // vextractf64x4 $0x1,%zmm0,%ymm1
        let b = [0x62, 0xf3, 0xfd, 0x48, 0x1b, 0xc1, 0x01];
        match rewrite(&dec(&b), &b).unwrap() {
            Rewrite::Thunk(seq) => {
                assert_eq!(
                    &seq[..7],
                    &[0x62, 0xf3, 0x79, 0x08, 0x07, 0xc8, 0x4e],
                    "vpermf32x4 zmm1, zmm0, 0x4e (map 0F3A)"
                );
                assert!(seq.len() > 20, "then the masked zero of the upper lanes");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn zeroing_and_narrow_forms_are_refused() {
        let z = [0x62, 0xf1, 0x74, 0xc9, 0x58, 0xc2];
        assert!(rewrite(&dec(&z), &z).unwrap_err().reason.contains("zeroing"));
        let ymm = [0x62, 0xf1, 0x74, 0x29, 0x58, 0xc2];
        assert!(rewrite(&dec(&ymm), &ymm).unwrap_err().reason.contains("256-bit"));
    }

    #[test]
    fn thunk_layout_jumps_there_and_back() {
        let (t, s) = thunk_bytes(&[0x90, 0x90], 0x2000, 0x1000, 7, 0x1007);
        assert_eq!(s, vec![0xe9, 0xfb, 0x0f, 0x00, 0x00, 0x90, 0x90]);
        assert_eq!(t[2], 0xe9);
        let back = i32::from_le_bytes(t[3..7].try_into().unwrap());
        assert_eq!(0x2000 + 7 + back as i64, 0x1007);
    }
}
