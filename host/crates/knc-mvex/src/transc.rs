//! The two transcendental approximations a SwiGLU needs, which Knights
//! Corner has as single instructions and AVX-512 does not (its nearest are
//! the AVX-512ER forms of Knights Landing, which are different encodings).
//! See transc.md.

use crate::{mask_text, mvex, src_rm, Insn, Map, Pp, Src, Zmm, K};

/// `vexp223ps zmm1 {k}, zmm2/mt`: 2 to the power of each int32 lane, read
/// as a fixed-point number with 24 fraction bits, as float32 with 0.99 ULP
/// relative error (MVEX.512.66.0F38.W0 C8 /r; ISA reference 327364-001,
/// page 190). The input comes from `vcvtfxpntps2dq` with the exponent
/// adjustment `ExpAdj::Q8_24`, which is Intel's two-instruction exp2; its
/// saturation is what makes the pair safe at any input (INT_MAX gives
/// +inf, INT_MIN gives +0, same page).
///
/// Register operands only: the page's swizzle table has no form but "no
/// swizzle" for this instruction, so a conversion or broadcast source is
/// refused here rather than encoded into an invalid opcode.
pub fn vexp223ps(dst: Zmm, src: Zmm, k: K) -> Insn {
    let src = Src::Reg(src);
    Insn {
        bytes: mvex(Map::M0F38, Pp::P66, false, dst.0, 0, src_rm(src), k.0, 0xc8, None),
        text: format!("vexp223ps {dst}{}, {src}", mask_text(k)),
    }
}

/// `vrcp23ps zmm1 {k}, zmm2/mt`: the reciprocal of each float32 lane with
/// 0.912 ULP relative error (MVEX.512.66.0F38.W0 CA /r; ISA reference
/// 327364-001, page 577). Plus or minus infinity gives 0 and a zero gives
/// infinity of its sign, which is what a sigmoid wants at its ends.
///
/// The page says any SwizzUpConv other than "no broadcast and no
/// conversion" raises an invalid-opcode exception, so like `vexp223ps`
/// this takes a register only.
pub fn vrcp23ps(dst: Zmm, src: Zmm, k: K) -> Insn {
    let src = Src::Reg(src);
    Insn {
        bytes: mvex(Map::M0F38, Pp::P66, false, dst.0, 0, src_rm(src), k.0, 0xca, None),
        text: format!("vrcp23ps {dst}{}, {src}", mask_text(k)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp2_and_reciprocal_share_the_0f38_66_row() {
        // 62, then P0 with R X B R' inverted and mm = 10 (0F38), P1 with
        // W 0, vvvv 1111 (none), 1, pp 01 (66), P2 with EH 0, SSS 000,
        // V' 1, aaa 000; then the opcode and a register ModRM.
        let e = vexp223ps(Zmm(1), Zmm(2), K(0));
        assert_eq!(e.bytes, vec![0x62, 0xf2, 0x79, 0x08, 0xc8, 0xca]);
        assert_eq!(e.text, "vexp223ps zmm1, zmm2");
        let r = vrcp23ps(Zmm(3), Zmm(4), K(0));
        assert_eq!(r.bytes, vec![0x62, 0xf2, 0x79, 0x08, 0xca, 0xdc]);
        assert_eq!(r.text, "vrcp23ps zmm3, zmm4");
    }

    #[test]
    fn a_mask_lands_in_aaa() {
        let e = vexp223ps(Zmm(0), Zmm(0), K(3));
        assert_eq!(e.bytes[3] & 7, 3);
    }
}
