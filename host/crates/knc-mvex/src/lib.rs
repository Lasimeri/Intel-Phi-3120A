//! Encoder for a subset of the Knights Corner vector instruction set.
//!
//! No assembler in this stack knows the card's 512-bit vector unit: its
//! instructions use the MVEX prefix, a four-byte prefix starting with 62H
//! that predates and differs from AVX-512's EVEX (ISA reference 327364-001,
//! section 3.3). This crate produces the bytes for the handful of
//! instructions the project needs (full-vector loads and stores, float64
//! add, subtract, multiply, fused multiply-add, compare into a mask, and
//! the mask register moves) so that they can be emitted as `.byte` lines
//! into otherwise ordinary assembly.
//!
//! Layout of the prefix, as used by Intel's k1om kernel macros (which the
//! tests in this file reproduce byte for byte):
//!
//! ```text
//! byte 0  62H
//! P0      R' R X B  0 0 m m      register extension bits, inverted; mm = opcode map
//! P1      W v v v v 0 p p        vvvv = first source, inverted; bit 2 is 0 (EVEX has 1); pp = prefix
//! P2      E S S S V' a a a       E = eviction hint, SSS = swizzle/conversion, V' = vvvv bit 4 inverted, aaa = mask
//! ```
//!
//! Only the plain forms are encoded: no swizzle, no conversion, no
//! eviction hint, memory operands as `[base + disp32]`. See lib.md.

use std::fmt;

/// A 512-bit vector register, `zmm0` to `zmm31`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zmm(pub u8);

/// A vector mask register, `k0` to `k7`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct K(pub u8);

/// A general purpose register, numbered as in the ModRM encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Gpr {
    Rax = 0,
    Rcx = 1,
    Rdx = 2,
    Rbx = 3,
    Rsp = 4,
    Rbp = 5,
    Rsi = 6,
    Rdi = 7,
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,
}

/// A memory operand `[base + disp]`, always encoded with a 32-bit
/// displacement (mod = 10), so no disp8*N scaling is involved. The base may
/// not be `rsp` or `r12`, which would need a SIB byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mem {
    pub base: Gpr,
    pub disp: i32,
}

impl Mem {
    pub fn new(base: Gpr, disp: i32) -> Mem {
        assert!(
            base as u8 & 7 != 4,
            "rsp and r12 as a base need a SIB byte, which this encoder does not emit"
        );
        Mem { base, disp }
    }
}

/// The second source of a three-operand instruction: a register or memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    Reg(Zmm),
    Mem(Mem),
}

/// Predicates of `vcmppd` (ISA reference, table 6.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Cmp {
    Eq = 0,
    Lt = 1,
    Le = 2,
    Unord = 3,
    Neq = 4,
    Nlt = 5,
    Nle = 6,
    Ord = 7,
}

/// One encoded instruction with its Intel-syntax text for the comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insn {
    pub bytes: Vec<u8>,
    pub text: String,
}

impl Insn {
    /// The instruction as a `.byte` directive with the mnemonic as comment.
    pub fn gas(&self) -> String {
        let bytes: Vec<String> = self.bytes.iter().map(|b| format!("0x{b:02x}")).collect();
        format!("\t.byte {}\t# {}", bytes.join(", "), self.text)
    }

    /// The instruction as a C string literal for inline assembly.
    pub fn c_string(&self) -> String {
        let bytes: Vec<String> = self.bytes.iter().map(|b| format!("0x{b:02x}")).collect();
        format!("\".byte {}\\n\\t\" /* {} */", bytes.join(","), self.text)
    }
}

impl fmt::Display for Zmm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "zmm{}", self.0)
    }
}

impl fmt::Display for K {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "k{}", self.0)
    }
}

impl fmt::Display for Gpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const NAMES: [&str; 16] = [
            "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15",
        ];
        f.write_str(NAMES[*self as usize])
    }
}

impl fmt::Display for Mem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}+{}]", self.base, self.disp)
    }
}

impl fmt::Display for Src {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Src::Reg(z) => write!(f, "{z}"),
            Src::Mem(m) => write!(f, "{m}"),
        }
    }
}

/// Opcode maps, the `mm` field of P0.
#[derive(Clone, Copy)]
enum Map {
    M0F = 1,
    M0F38 = 2,
}

/// Legacy prefix compaction, the `pp` field of P1.
#[derive(Clone, Copy)]
enum Pp {
    None = 0,
    P66 = 1,
}

/// The r/m operand of the ModRM byte.
#[derive(Clone, Copy)]
enum Rm {
    Zmm(Zmm),
    Mem(Mem),
}

/// Assemble one MVEX instruction. `reg` is the ModRM.reg operand (with its
/// R and R' extensions), `vvvv` the first source, `rm` the r/m operand,
/// `aaa` the write mask.
#[allow(clippy::too_many_arguments)]
fn mvex(map: Map, pp: Pp, w: bool, reg: u8, vvvv: u8, rm: Rm, aaa: u8, opcode: u8, imm: Option<u8>) -> Vec<u8> {
    assert!(reg < 32 && vvvv < 32 && aaa < 8);
    let (b, x) = match rm {
        Rm::Zmm(z) => {
            assert!(z.0 < 32);
            (z.0 >> 3 & 1, z.0 >> 4 & 1)
        }
        Rm::Mem(m) => (m.base as u8 >> 3 & 1, 0),
    };
    let p0 = (!(reg >> 3 & 1) & 1) << 7 | (!x & 1) << 6 | (!b & 1) << 5 | (!(reg >> 4 & 1) & 1) << 4 | map as u8;
    let p1 = (w as u8) << 7 | (!vvvv & 0xf) << 3 | pp as u8;
    let p2 = (!(vvvv >> 4) & 1) << 3 | aaa;
    let mut out = vec![0x62, p0, p1, p2, opcode];
    match rm {
        Rm::Zmm(z) => out.push(0xc0 | (reg & 7) << 3 | z.0 & 7),
        Rm::Mem(m) => {
            out.push(0x80 | (reg & 7) << 3 | m.base as u8 & 7);
            out.extend_from_slice(&m.disp.to_le_bytes());
        }
    }
    if let Some(i) = imm {
        out.push(i);
    }
    out
}

fn src_rm(src: Src) -> Rm {
    match src {
        Src::Reg(z) => Rm::Zmm(z),
        Src::Mem(m) => Rm::Mem(m),
    }
}

fn mask_text(k: K) -> String {
    if k.0 == 0 {
        String::new()
    } else {
        format!(" {{{k}}}")
    }
}

/// `vmovaps mt, zmm`: store 64 bytes. The form Intel's kernel uses.
pub fn vmovaps_store(mem: Mem, src: Zmm) -> Insn {
    Insn {
        bytes: mvex(Map::M0F, Pp::None, false, src.0, 0, Rm::Mem(mem), 0, 0x29, None),
        text: format!("vmovaps {mem}, {src}"),
    }
}

/// `vmovaps zmm, mt`: load 64 bytes.
pub fn vmovaps_load(dst: Zmm, mem: Mem) -> Insn {
    Insn {
        bytes: mvex(Map::M0F, Pp::None, false, dst.0, 0, Rm::Mem(mem), 0, 0x28, None),
        text: format!("vmovaps {dst}, {mem}"),
    }
}

/// `vmovapd zmm1 {k}, zmm2/mt`: float64 vector move (MVEX.512.66.0F.W1 28).
pub fn vmovapd_load(dst: Zmm, src: Src, k: K) -> Insn {
    Insn {
        bytes: mvex(Map::M0F, Pp::P66, true, dst.0, 0, src_rm(src), k.0, 0x28, None),
        text: format!("vmovapd {dst}{}, {src}", mask_text(k)),
    }
}

/// `vmovapd mt {k}, zmm1`: float64 vector store (MVEX.512.66.0F.W1 29).
pub fn vmovapd_store(mem: Mem, src: Zmm, k: K) -> Insn {
    Insn {
        bytes: mvex(Map::M0F, Pp::P66, true, src.0, 0, Rm::Mem(mem), k.0, 0x29, None),
        text: format!("vmovapd {mem}{}, {src}", mask_text(k)),
    }
}

fn arith(name: &str, map: Map, opcode: u8, dst: Zmm, src1: Zmm, src2: Src, k: K) -> Insn {
    Insn {
        bytes: mvex(map, Pp::P66, true, dst.0, src1.0, src_rm(src2), k.0, opcode, None),
        text: format!("{name} {dst}{}, {src1}, {src2}", mask_text(k)),
    }
}

/// `vaddpd zmm1 {k}, zmm2, zmm3/mt` (MVEX.NDS.512.66.0F.W1 58).
pub fn vaddpd(dst: Zmm, src1: Zmm, src2: Src, k: K) -> Insn {
    arith("vaddpd", Map::M0F, 0x58, dst, src1, src2, k)
}

/// `vsubpd zmm1 {k}, zmm2, zmm3/mt` (MVEX.NDS.512.66.0F.W1 5C).
pub fn vsubpd(dst: Zmm, src1: Zmm, src2: Src, k: K) -> Insn {
    arith("vsubpd", Map::M0F, 0x5c, dst, src1, src2, k)
}

/// `vmulpd zmm1 {k}, zmm2, zmm3/mt` (MVEX.NDS.512.66.0F.W1 59).
pub fn vmulpd(dst: Zmm, src1: Zmm, src2: Src, k: K) -> Insn {
    arith("vmulpd", Map::M0F, 0x59, dst, src1, src2, k)
}

/// `vfmadd213pd zmm1 {k}, zmm2, zmm3/mt`: zmm1 = zmm2 * zmm1 + zmm3
/// (MVEX.NDS.512.66.0F38.W1 A8).
pub fn vfmadd213pd(dst: Zmm, src1: Zmm, src2: Src, k: K) -> Insn {
    arith("vfmadd213pd", Map::M0F38, 0xa8, dst, src1, src2, k)
}

/// `vfmadd231pd zmm1 {k}, zmm2, zmm3/mt`: zmm1 = zmm2 * zmm3 + zmm1
/// (MVEX.NDS.512.66.0F38.W1 B8).
pub fn vfmadd231pd(dst: Zmm, src1: Zmm, src2: Src, k: K) -> Insn {
    arith("vfmadd231pd", Map::M0F38, 0xb8, dst, src1, src2, k)
}

/// `vcmppd k2 {k1}, zmm1, zmm2/mt, imm8`: element compare into a mask. A
/// zero bit in the write mask k1 clears the result bit, so `k2 = k1 & cmp`
/// (MVEX.NDS.512.66.0F.W1 C2 /r ib).
pub fn vcmppd(dst: K, src1: Zmm, src2: Src, pred: Cmp, k: K) -> Insn {
    Insn {
        bytes: mvex(Map::M0F, Pp::P66, true, dst.0, src1.0, src_rm(src2), k.0, 0xc2, Some(pred as u8)),
        text: format!("vcmppd {dst}{}, {src1}, {src2}, {}", mask_text(k), pred as u8),
    }
}

/// Assemble a two-byte VEX instruction of the mask register family
/// (VEX.128.0F.W0, no legacy prefix).
fn vex_k(opcode: u8, reg: u8, rm: u8) -> Vec<u8> {
    assert!(reg < 8 && rm < 8, "only registers 0 to 7 without REX extension");
    vec![0xc5, 0xf8, opcode, 0xc0 | reg << 3 | rm]
}

/// `kmov r32, k` (VEX.128.0F.W0 93 /r).
pub fn kmov_r32_k(dst: Gpr, src: K) -> Insn {
    Insn {
        bytes: vex_k(0x93, dst as u8, src.0),
        text: format!("kmov {}, {src}", gpr32(dst)),
    }
}

/// `kmov k, r32` (VEX.128.0F.W0 92 /r).
pub fn kmov_k_r32(dst: K, src: Gpr) -> Insn {
    Insn {
        bytes: vex_k(0x92, dst.0, src as u8),
        text: format!("kmov {dst}, {}", gpr32(src)),
    }
}

/// `kmov k1, k2` (VEX.128.0F.W0 90 /r).
pub fn kmov_k_k(dst: K, src: K) -> Insn {
    Insn {
        bytes: vex_k(0x90, dst.0, src.0),
        text: format!("kmov {dst}, {src}"),
    }
}

/// `kortest k1, k2`: ZF = ((k1 | k2) == 0), CF = ((k1 | k2) == all ones)
/// (VEX.128.0F.W0 98 /r).
pub fn kortest(a: K, b: K) -> Insn {
    Insn {
        bytes: vex_k(0x98, a.0, b.0),
        text: format!("kortest {a}, {b}"),
    }
}

fn gpr32(g: Gpr) -> &'static str {
    const NAMES: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
    NAMES[g as usize & 7]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Intel's k1om kernel macro VSTORED_DISP32_EAX(v, disp32):
    /// `.byte 0x62, 0xf1 ^ (v & 0x10) ^ ((v & 0x8) << 4), 0x78, 0x08, 0x29,
    /// 0x80 + ((v & 0x7) << 3); .long disp32` (mic_ni.h, reference tree).
    fn intel_vstored(v: u8, disp: i32) -> Vec<u8> {
        let mut b = vec![0x62, 0xf1 ^ (v & 0x10) ^ ((v & 0x8) << 4), 0x78, 0x08, 0x29, 0x80 + ((v & 7) << 3)];
        b.extend_from_slice(&disp.to_le_bytes());
        b
    }

    fn intel_vloadd(v: u8, disp: i32) -> Vec<u8> {
        let mut b = vec![0x62, 0xf1 ^ (v & 0x10) ^ ((v & 0x8) << 4), 0x78, 0x08, 0x28, 0x80 + ((v & 7) << 3)];
        b.extend_from_slice(&disp.to_le_bytes());
        b
    }

    #[test]
    fn vector_store_matches_intel_macro() {
        for v in 0..32u8 {
            let disp = i32::from(v) * 64;
            assert_eq!(
                vmovaps_store(Mem::new(Gpr::Rax, disp), Zmm(v)).bytes,
                intel_vstored(v, disp),
                "zmm{v}"
            );
        }
    }

    #[test]
    fn vector_load_matches_intel_macro() {
        for v in [0u8, 1, 7, 8, 15, 16, 23, 24, 31] {
            assert_eq!(
                vmovaps_load(Zmm(v), Mem::new(Gpr::Rax, 0x7c0)).bytes,
                intel_vloadd(v, 0x7c0),
                "zmm{v}"
            );
        }
    }

    /// VKMOV_TO_EBX(k): `.byte 0xc5, 0xf8, 0x93, 0xd8 + k`;
    /// VKMOV_FROM_EBX(k): `.byte 0xc5, 0xf8, 0x92, 0xc3 + (k << 3)`.
    #[test]
    fn mask_moves_match_intel_macros() {
        for k in 0..8u8 {
            assert_eq!(kmov_r32_k(Gpr::Rbx, K(k)).bytes, vec![0xc5, 0xf8, 0x93, 0xd8 + k]);
            assert_eq!(kmov_k_r32(K(k), Gpr::Rbx).bytes, vec![0xc5, 0xf8, 0x92, 0xc3 + (k << 3)]);
        }
    }

    #[test]
    fn float64_forms_set_w_and_66() {
        // vmovapd zmm0, [rdi+0]: P1 = W1, vvvv unused (1111), bit 2 clear, pp = 66.
        let i = vmovapd_load(Zmm(0), Src::Mem(Mem::new(Gpr::Rdi, 0)), K(0));
        assert_eq!(&i.bytes[..6], &[0x62, 0xf1, 0xf9, 0x08, 0x28, 0x87]);
        // vaddpd zmm2 {k1}, zmm0, zmm1: vvvv = ~0 = 1111, aaa = 001, ModRM 11 010 001.
        let i = vaddpd(Zmm(2), Zmm(0), Src::Reg(Zmm(1)), K(1));
        assert_eq!(i.bytes, vec![0x62, 0xf1, 0xf9, 0x09, 0x58, 0xd1]);
        // vmulpd zmm11 {k1}, zmm2, zmm7: reg 11 -> R clear; vvvv = ~2 = 1101.
        let i = vmulpd(Zmm(11), Zmm(2), Src::Reg(Zmm(7)), K(1));
        assert_eq!(i.bytes, vec![0x62, 0x71, 0xe9, 0x09, 0x59, 0xdf]);
    }

    #[test]
    fn high_registers_use_extension_bits() {
        // zmm31 as destination: R and R' clear; zmm16 as vvvv: V' clear; zmm24 in rm: B and X clear.
        let i = vaddpd(Zmm(31), Zmm(16), Src::Reg(Zmm(24)), K(0));
        assert_eq!(i.bytes, vec![0x62, 0x01, 0xf9, 0x00, 0x58, 0xf8]);
    }

    #[test]
    fn compare_carries_predicate() {
        let i = vcmppd(K(1), Zmm(10), Src::Reg(Zmm(8)), Cmp::Lt, K(1));
        assert_eq!(i.bytes, vec![0x62, 0xd1, 0xa9, 0x09, 0xc2, 0xc8, 0x01]); // zmm8 in r/m: B clear
        assert_eq!(kortest(K(1), K(1)).bytes, vec![0xc5, 0xf8, 0x98, 0xc9]);
    }

    #[test]
    fn gas_line_format() {
        let i = kmov_k_r32(K(1), Gpr::Rax);
        assert_eq!(i.gas(), "\t.byte 0xc5, 0xf8, 0x92, 0xc8\t# kmov k1, eax");
    }
}
