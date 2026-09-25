//! Memory operands read through the card's up-conversions and broadcasts,
//! and the instructions the quantized matrix-multiply kernels need beyond
//! the plain forms of lib.rs: integer to float and float rounding, the
//! negated and subtracting fused multiply-adds, and the int32 move. See
//! conv.md.

use std::fmt;

use crate::{mask_text, mvex, src_rm, src_sss, Insn, Map, Mem, Pp, Rm, Src, Zmm, K};

/// The SSS field of P2 for a memory operand (ISA reference tables 2.9 and
/// 2.10; the values are those of Intel's `_MM_UPCONV_PS_*` and
/// `_MM_UPCONV_EPI32_*` enumerations). `Bcast1` reads one element and
/// fills the sixteen lanes with it; `Bcast4` reads four and repeats them;
/// the byte and word forms read sixteen elements of that width and widen
/// each to its 32-bit lane, to a float for the float32 instructions and to
/// an integer for the int32 ones; `F16` is the float16 to float32
/// conversion the float32 loads offer. A converted operand is aligned to
/// its own size in memory (16 bytes for sixteen bytes, 4 for a broadcast
/// scalar), not to 64.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Conv {
    None = 0,
    Bcast1 = 1,
    Bcast4 = 2,
    F16 = 3,
    U8 = 4,
    S8 = 5,
    U16 = 6,
    S16 = 7,
}

impl fmt::Display for Conv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Conv::None => "",
            Conv::Bcast1 => "{1to16}",
            Conv::Bcast4 => "{4to16}",
            Conv::F16 => "{float16}",
            Conv::U8 => "{uint8}",
            Conv::S8 => "{sint8}",
            Conv::U16 => "{uint16}",
            Conv::S16 => "{sint16}",
        })
    }
}

/// Set the SSS field of an assembled instruction's P2 byte.
pub(crate) fn with_sss(mut bytes: Vec<u8>, sss: u8) -> Vec<u8> {
    debug_assert!(sss < 8);
    bytes[3] |= sss << 4;
    bytes
}

/// `vmovaps zmm1 {k}, Uf32(mt)`: a float32 load through a conversion
/// (MVEX.512.0F.W0 28 with SSS = `conv`). `Conv::None` is the plain
/// 64-byte aligned load.
pub fn vmovaps_load_conv(dst: Zmm, mem: Mem, conv: Conv, k: K) -> Insn {
    Insn {
        bytes: with_sss(mvex(Map::M0F, Pp::None, false, dst.0, 0, Rm::Mem(mem), k.0, 0x28, None), conv as u8),
        text: format!("vmovaps {dst}{}, {mem}{conv}", mask_text(k)),
    }
}

/// `vmovdqa32 zmm1 {k}, Ui32(mt)`: an int32 load through a conversion
/// (MVEX.512.66.0F.W0 6F).
pub fn vmovdqa32_load_conv(dst: Zmm, mem: Mem, conv: Conv, k: K) -> Insn {
    Insn {
        bytes: with_sss(mvex(Map::M0F, Pp::P66, false, dst.0, 0, Rm::Mem(mem), k.0, 0x6f, None), conv as u8),
        text: format!("vmovdqa32 {dst}{}, {mem}{conv}", mask_text(k)),
    }
}

/// The unaligned load pair of int32 lanes through a conversion
/// (`vloadunpackld` / `vloadunpackhd`, MVEX.512.0F38.W0 D0 / D4, with SSS =
/// `conv`): sixteen bytes or words from any address become sixteen
/// integers. The float32 twin is `vloadunpacklps_conv` (D1 / D5); the
/// probe of `phi-vpu matmul-check --probe` showed this form delivering
/// `{uint8}` as the integers 0, 1, 2, 3, which is what the ISA says
/// (VLOADUNPACKLD takes the Ui32 conversions). The high half's address
/// is 64 above the low one's, as for the plain pair. Broadcasts are not
/// valid on an unpack.
pub fn vloadunpackld_conv(dst: Zmm, mem: Mem, conv: Conv, k: K) -> Insn {
    assert!(!matches!(conv, Conv::Bcast1 | Conv::Bcast4), "no broadcast on an unpack load");
    Insn {
        bytes: with_sss(
            mvex(Map::M0F38, Pp::None, false, dst.0, 0, Rm::Mem(mem), k.0, 0xd0, None),
            conv as u8,
        ),
        text: format!("vloadunpackld {dst}{}, {mem}{conv}", mask_text(k)),
    }
}

pub fn vloadunpackhd_conv(dst: Zmm, mem: Mem, conv: Conv, k: K) -> Insn {
    assert!(!matches!(conv, Conv::Bcast1 | Conv::Bcast4), "no broadcast on an unpack load");
    Insn {
        bytes: with_sss(
            mvex(Map::M0F38, Pp::None, false, dst.0, 0, Rm::Mem(mem), k.0, 0xd4, None),
            conv as u8,
        ),
        text: format!("vloadunpackhd {dst}{}, {mem}{conv}", mask_text(k)),
    }
}

/// `vcvtfxpntdq2ps zmm1 {k}, zmm2/mt, imm8`: int32 to float32
/// (MVEX.512.0F3A.W0 CB /r ib, no legacy prefix; the 66-prefixed CB is the
/// float to int direction). The immediate is 0: round to nearest, no
/// exponent adjustment. The same encoding the rewriter uses for
/// `vcvtdq2ps` (avx512-xlate, verified against hardware results).
pub fn vcvtfxpntdq2ps(dst: Zmm, src: Src, k: K) -> Insn {
    Insn {
        bytes: with_sss(
            mvex(Map::M0F3A, Pp::None, false, dst.0, 0, src_rm(src), k.0, 0xcb, Some(0)),
            src_sss(src),
        ),
        text: format!("vcvtfxpntdq2ps {dst}{}, {src}, 0", mask_text(k)),
    }
}

/// Rounding modes of `vrndfxpntps` (the two low bits of its immediate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Round {
    Nearest = 0,
    Down = 1,
    Up = 2,
    Zero = 3,
}

/// `vrndfxpntps zmm1 {k}, zmm2/mt, imm8`: round each float32 to an
/// integer value in the given mode (MVEX.512.66.0F3A.W0 52 /r ib; the
/// high bits of the immediate, an exponent adjustment, stay 0). The
/// rewriter's translation of `vrndscaleps`.
pub fn vrndfxpntps(dst: Zmm, src: Src, mode: Round, k: K) -> Insn {
    Insn {
        bytes: with_sss(
            mvex(Map::M0F3A, Pp::P66, false, dst.0, 0, src_rm(src), k.0, 0x52, Some(mode as u8)),
            src_sss(src),
        ),
        text: format!("vrndfxpntps {dst}{}, {src}, {}", mask_text(k), mode as u8),
    }
}

/// The FMA family of lib.rs continued; all MVEX.NDS.512.66.0F38.W0, the
/// opcodes those of AVX-512.
fn fma(name: &str, opcode: u8, dst: Zmm, src1: Zmm, src2: Src, k: K) -> Insn {
    Insn {
        bytes: with_sss(
            mvex(Map::M0F38, Pp::P66, false, dst.0, src1.0, src_rm(src2), k.0, opcode, None),
            src_sss(src2),
        ),
        text: format!("{name} {dst}{}, {src1}, {src2}", mask_text(k)),
    }
}

/// `vfnmadd231ps zmm1 {k}, zmm2, zmm3/mt`: zmm1 = -(zmm2 * src) + zmm1 (BC).
pub fn vfnmadd231ps(dst: Zmm, src1: Zmm, src2: Src, k: K) -> Insn {
    fma("vfnmadd231ps", 0xbc, dst, src1, src2, k)
}

/// `vfmsub213ps zmm1 {k}, zmm2, zmm3/mt`: zmm1 = zmm2 * zmm1 - src (AA).
pub fn vfmsub213ps(dst: Zmm, src1: Zmm, src2: Src, k: K) -> Insn {
    fma("vfmsub213ps", 0xaa, dst, src1, src2, k)
}

/// `vfmsub231ps zmm1 {k}, zmm2, zmm3/mt`: zmm1 = zmm2 * src - zmm1 (BA).
pub fn vfmsub231ps(dst: Zmm, src1: Zmm, src2: Src, k: K) -> Insn {
    fma("vfmsub231ps", 0xba, dst, src1, src2, k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{vfmadd231ps, vmovaps_load_f16, Gpr};

    #[test]
    fn f16_conversion_matches_the_older_form() {
        let a = vmovaps_load_conv(Zmm(4), Mem::new(Gpr::Rdi, 32), Conv::F16, K(0));
        let b = vmovaps_load_f16(Zmm(4), Mem::new(Gpr::Rdi, 32));
        assert_eq!(a.bytes, b.bytes);
    }

    #[test]
    fn conversion_lands_in_sss() {
        // vmovaps zmm4, [rdi+0] {uint8}: P2 = SSS 100, V' 1, aaa 000
        let i = vmovaps_load_conv(Zmm(4), Mem::new(Gpr::Rdi, 0), Conv::U8, K(0));
        assert_eq!(&i.bytes[..6], &[0x62, 0xf1, 0x78, 0x48, 0x28, 0xa7]);
        // sint8 into an int32 move
        let i = vmovdqa32_load_conv(Zmm(0), Mem::new(Gpr::Rsi, 0), Conv::S8, K(0));
        assert_eq!(&i.bytes[..6], &[0x62, 0xf1, 0x79, 0x58, 0x6f, 0x86]);
    }

    #[test]
    fn broadcast_source_matches_the_rewriter() {
        // vfmadd231ps zmm0, zmm1, [rcx+0]{1to16}: the rewriter's test has P2 = 0x18 for {1to16}
        let i = vfmadd231ps(Zmm(0), Zmm(1), Src::MemConv(Mem::new(Gpr::Rcx, 0), Conv::Bcast1), K(0));
        assert_eq!(&i.bytes[..6], &[0x62, 0xf2, 0x71, 0x18, 0xb8, 0x81]);
        assert_eq!(i.text, "vfmadd231ps zmm0, zmm1, [rcx+0]{1to16}");
    }

    #[test]
    fn conversions_and_rounding() {
        // vcvtfxpntdq2ps zmm2, zmm3, 0: map 0F3A (mm = 11), no prefix
        let i = vcvtfxpntdq2ps(Zmm(2), Src::Reg(Zmm(3)), K(0));
        assert_eq!(i.bytes, vec![0x62, 0xf3, 0x78, 0x08, 0xcb, 0xd3, 0x00]);
        // vrndfxpntps zmm5, zmm6, 1 (round down): 66 prefix
        let i = vrndfxpntps(Zmm(5), Src::Reg(Zmm(6)), Round::Down, K(0));
        assert_eq!(i.bytes, vec![0x62, 0xf3, 0x79, 0x08, 0x52, 0xee, 0x01]);
    }

    #[test]
    fn permute_and_float_to_int() {
        let i = vpermd(Zmm(1), Zmm(2), Src::Reg(Zmm(3)), K(0));
        assert_eq!(i.bytes, vec![0x62, 0xf2, 0x69, 0x08, 0x36, 0xcb]);
        let i = vcvtfxpntps2dq(Zmm(2), Src::Reg(Zmm(3)), Round::Zero, K(0));
        assert_eq!(i.bytes, vec![0x62, 0xf3, 0x79, 0x08, 0xcb, 0xd3, 0x03]);
    }

    #[test]
    fn a_converting_store_is_the_plain_store_plus_sss() {
        use crate::vmovaps_store;
        let plain = vmovaps_store(Mem::new(Gpr::Rdx, 32), Zmm(5));
        let none = vmovaps_store_conv(Mem::new(Gpr::Rdx, 32), Zmm(5), Conv::None, K(0));
        assert_eq!(plain.bytes, none.bytes);
        // {float16} is SSS 011 in P2, a mask its aaa, nothing else moves
        let half = vmovaps_store_conv(Mem::new(Gpr::Rdx, 32), Zmm(5), Conv::F16, K(1));
        assert_eq!(half.bytes[3], plain.bytes[3] | 0x30 | 1);
        assert_eq!(&half.bytes[4..], &plain.bytes[4..]);
        assert_eq!(half.text, "vmovaps [rdx+32] {k1}{float16}, zmm5");
    }

    #[test]
    fn exponent_adjustment_lands_in_the_high_nibble() {
        // 8.24 fixed point is I6..I4 = 101, round to nearest I1..I0 = 00: 0x50
        let i = vcvtfxpntps2dq_adj(Zmm(2), Src::Reg(Zmm(3)), Round::Nearest, ExpAdj::Q8_24, K(0));
        assert_eq!(i.bytes, vec![0x62, 0xf3, 0x79, 0x08, 0xcb, 0xd3, 0x50]);
        // no adjustment is the plain form, byte for byte
        let plain = vcvtfxpntps2dq(Zmm(2), Src::Reg(Zmm(3)), Round::Zero, K(0));
        let adj = vcvtfxpntps2dq_adj(Zmm(2), Src::Reg(Zmm(3)), Round::Zero, ExpAdj::None, K(0));
        assert_eq!(plain.bytes, adj.bytes);
    }

    #[test]
    fn unpack_forms() {
        // int32 form D0, float32 form D1, both unprefixed; the store twin carries 66
        assert_eq!(
            &vloadunpackld_conv(Zmm(8), Mem::new(Gpr::Rdi, 3), Conv::U8, K(0)).bytes[..6],
            &[0x62, 0x72, 0x78, 0x48, 0xd0, 0x87]
        );
        assert_eq!(
            &vloadunpacklps_conv(Zmm(8), Mem::new(Gpr::Rdi, 3), Conv::U8, K(0)).bytes[..6],
            &[0x62, 0x72, 0x78, 0x48, 0xd1, 0x87]
        );
        assert_eq!(
            &vloadunpackhps_conv(Zmm(8), Mem::new(Gpr::Rdi, 67), Conv::S8, K(0)).bytes[..6],
            &[0x62, 0x72, 0x78, 0x58, 0xd5, 0x87]
        );
    }

    #[test]
    fn fma_opcodes() {
        assert_eq!(vfnmadd231ps(Zmm(0), Zmm(1), Src::Reg(Zmm(2)), K(0)).bytes[4], 0xbc);
        assert_eq!(vfmsub213ps(Zmm(0), Zmm(1), Src::Reg(Zmm(2)), K(0)).bytes[4], 0xaa);
        assert_eq!(vfmsub231ps(Zmm(0), Zmm(1), Src::Reg(Zmm(2)), K(0)).bytes[4], 0xba);
    }
}

/// `vpermd zmm1 {k}, zmm2, zmm3/mt`: each lane of the result is the lane of
/// `src2` that the low four bits of the corresponding lane of `zmm2`
/// select (MVEX.NDS.512.66.0F38.W0 36 /r, the opcode AVX-512 kept). A
/// sixteen-entry table in `src2` and indices in `zmm2` make it a lookup.
pub fn vpermd(dst: Zmm, idx: Zmm, src2: Src, k: K) -> Insn {
    Insn {
        bytes: with_sss(
            mvex(Map::M0F38, Pp::P66, false, dst.0, idx.0, src_rm(src2), k.0, 0x36, None),
            src_sss(src2),
        ),
        text: format!("vpermd {dst}{}, {idx}, {src2}", mask_text(k)),
    }
}

/// `vcvtfxpntps2dq zmm1 {k}, zmm2/mt, imm8`: float32 to int32 in the
/// rounding mode of the immediate (MVEX.512.66.0F3A.W0 CB /r ib, the
/// 66-prefixed twin of `vcvtfxpntdq2ps`; the rewriter's `vcvtps2dq`).
pub fn vcvtfxpntps2dq(dst: Zmm, src: Src, mode: Round, k: K) -> Insn {
    Insn {
        bytes: with_sss(
            mvex(Map::M0F3A, Pp::P66, false, dst.0, 0, src_rm(src), k.0, 0xcb, Some(mode as u8)),
            src_sss(src),
        ),
        text: format!("vcvtfxpntps2dq {dst}{}, {src}, {}", mask_text(k), mode as u8),
    }
}

/// `vmovaps mt {k}, zmm {conv}`: store sixteen floats through a
/// down-conversion, under a write-mask (MVEX.512.0F.W0 29 /r with SSS the
/// `Df32` conversion; ISA reference 327364-001, page 379, and table 2.12).
/// `Conv::F16` writes sixteen halves (32 bytes, 32-byte aligned) rounded in
/// MXCSR.RC; a lane whose mask bit is clear is not written at all, so two
/// threads may each store their own lanes of one vector. The broadcast
/// values of SSS are reserved for stores and refused here.
pub fn vmovaps_store_conv(mem: Mem, src: Zmm, conv: Conv, k: K) -> Insn {
    assert!(conv != Conv::Bcast1 && conv != Conv::Bcast4, "001 and 010 are reserved for stores");
    Insn {
        bytes: with_sss(mvex(Map::M0F, Pp::None, false, src.0, 0, Rm::Mem(mem), k.0, 0x29, None), conv as u8),
        text: format!("vmovaps {mem}{}{conv}, {src}", mask_text(k)),
    }
}

/// The exponent adjustment of `vcvtfxpntps2dq`: bits I6..I4 of its
/// immediate, which scale the float by 2^n before the conversion so the
/// integer it produces is a fixed-point number with n fraction bits (ISA
/// reference 327364-001, page 169, the "Exponent Adjustment" table; 1xxx
/// in I7..I4 is reserved and must raise an invalid opcode, so it has no
/// variant here). `Q8_24` is the one `vexp223ps` reads (page 190).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExpAdj {
    None = 0,
    Q28_4 = 1,
    Q27_5 = 2,
    Q24_8 = 3,
    Q16_16 = 4,
    Q8_24 = 5,
    Q1_31 = 6,
    Q0_32 = 7,
}

/// `vcvtfxpntps2dq` with an exponent adjustment: float32 to a fixed-point
/// int32 with `adj`'s fraction bits, rounded in `mode`, saturating (out of
/// range gives INT_MAX or INT_MIN and NaN gives 0, page 169). With
/// `ExpAdj::Q8_24` and then `vexp223ps` this is Intel's exp2.
pub fn vcvtfxpntps2dq_adj(dst: Zmm, src: Src, mode: Round, adj: ExpAdj, k: K) -> Insn {
    let imm = ((adj as u8) << 4) | mode as u8;
    Insn {
        bytes: with_sss(
            mvex(Map::M0F3A, Pp::P66, false, dst.0, 0, src_rm(src), k.0, 0xcb, Some(imm)),
            src_sss(src),
        ),
        text: format!("vcvtfxpntps2dq {dst}{}, {src}, {imm:#04x}", mask_text(k)),
    }
}

/// The unaligned load pair of float32 lanes through a conversion
/// (`vloadunpacklps` / `vloadunpackhps`, MVEX.512.0F38.W0 D1 / D5, the
/// Uf32 conversions): sixteen bytes from any address become sixteen
/// floats. The same opcodes with a 66 prefix are the pack-stores, which
/// the first probe of this file found out by overwriting its own block;
/// the rewriter (avx512-xlate, `load_unaligned`) uses D1 / D5 for its
/// float16 loads, checked against hardware.
pub fn vloadunpacklps_conv(dst: Zmm, mem: Mem, conv: Conv, k: K) -> Insn {
    Insn {
        bytes: with_sss(
            mvex(Map::M0F38, Pp::None, false, dst.0, 0, Rm::Mem(mem), k.0, 0xd1, None),
            conv as u8,
        ),
        text: format!("vloadunpacklps {dst}{}, {mem}{conv}", mask_text(k)),
    }
}

pub fn vloadunpackhps_conv(dst: Zmm, mem: Mem, conv: Conv, k: K) -> Insn {
    Insn {
        bytes: with_sss(
            mvex(Map::M0F38, Pp::None, false, dst.0, 0, Rm::Mem(mem), k.0, 0xd5, None),
            conv as u8,
        ),
        text: format!("vloadunpackhps {dst}{}, {mem}{conv}", mask_text(k)),
    }
}

/// Which cache a prefetch fills: `vprefetch0` the L1 (MVEX.512.0F.W0 18 /1),
/// `vprefetch1` the L2 (/2). The card's cores are in order and its L1 has
/// no hardware prefetcher, so a stream that is not prefetched stalls for
/// the whole GDDR latency on every line (Intel Xeon Phi Coprocessor
/// System Software Developers Guide, the cache section; measured here in
/// `docs/results/2026-09-23-quantized-kernels.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Cache {
    L1 = 1,
    L2 = 2,
}

/// `vprefetch0 mt` / `vprefetch1 mt`: fetch the cache line at `mt`. No
/// destination, no mask; the opcode extension is in ModRM.reg.
pub fn vprefetch(mem: Mem, cache: Cache) -> Insn {
    Insn {
        bytes: mvex(Map::M0F, Pp::None, false, cache as u8, 0, Rm::Mem(mem), 0, 0x18, None),
        text: format!("vprefetch{} {mem}", cache as u8 - 1),
    }
}
