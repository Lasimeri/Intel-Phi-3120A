//! Performing an AVX-512 instruction without an AVX-512 processor.
//!
//! Every operation here works on [`VState`], the imaginary register file,
//! and on the program's own memory, which is reachable because this runs
//! inside the program's address space.
//!
//! The arithmetic is written in plain Rust on `f32` and `f64` rather than
//! in host vector intrinsics. That is deliberate for this stage: these are
//! IEEE operations with the same rounding on both machines, so the scalar
//! form gives the same bits as the vector form, and it is much easier to
//! read and to be sure of. Speed comes from not reaching this code at all,
//! by rewriting the call site once its translation exists, rather than
//! from making the fallback clever.
//!
//! One exception is worth naming: `mul_add` is used for the fused
//! multiply-adds because it is a single rounding, exactly as the hardware
//! FMA is. Writing `a * b + c` there would round twice and produce
//! different bits.

use iced_x86::{Instruction, Mnemonic, OpKind, Register};

use crate::{effective_address, Gprs, VState};

/// An instruction this build cannot perform, and why. Returned rather than
/// ignored: skipping an AVX-512 instruction would leave the imaginary
/// register file disagreeing with what the program believes, and every
/// answer after that would be quietly wrong.
#[derive(Clone, Debug)]
pub struct Unsupported(pub String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Lane width of the operation, in bits, from the mnemonic's suffix.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lanes {
    F32,
    F64,
    I32,
    I64,
}

fn zmm_index(r: Register) -> Option<usize> {
    if r.is_zmm() {
        Some(r as usize - Register::ZMM0 as usize)
    } else if r.is_ymm() {
        Some(r as usize - Register::YMM0 as usize)
    } else if r.is_xmm() {
        Some(r as usize - Register::XMM0 as usize)
    } else {
        None
    }
}

fn mask_index(r: Register) -> usize {
    if r == Register::None {
        0
    } else {
        r as usize - Register::K0 as usize
    }
}

/// How many bytes of the destination the instruction actually writes. The
/// 128-bit and 256-bit EVEX forms (AVX-512VL) write less than 64 and zero
/// the rest, which is the architectural rule for a vector write.
fn dest_width(insn: &Instruction) -> usize {
    let r = insn.op0_register();
    if r.is_zmm() {
        64
    } else if r.is_ymm() {
        32
    } else if r.is_xmm() {
        16
    } else {
        64
    }
}

/// Read the second source, whatever kind it is, into 64 bytes.
///
/// A memory source with the broadcast bit set names one element, not a
/// vector: `{1to16}` means read four bytes and use them for every lane,
/// which is why this cannot simply copy 64 bytes from the address.
fn source_bytes(insn: &Instruction, op: u32, st: &VState, gprs: &dyn Gprs, lanes: Lanes) -> Result<[u8; 64], Unsupported> {
    let mut out = [0u8; 64];
    match insn.op_kind(op) {
        OpKind::Register => {
            let r = insn.op_register(op);
            let i = zmm_index(r).ok_or_else(|| Unsupported(format!("source operand {op} is {r:?}, not a vector register")))?;
            out.copy_from_slice(&st.zmm[i]);
        }
        OpKind::Memory => {
            let addr = effective_address(insn, gprs) as *const u8;
            let elem = match lanes {
                Lanes::F32 | Lanes::I32 => 4usize,
                Lanes::F64 | Lanes::I64 => 8usize,
            };
            // SAFETY: the address was computed from the program's own
            // registers for an access the program itself was about to
            // make. If it is bad, the program would have faulted anyway,
            // and it faults here in the same way.
            unsafe {
                if insn.is_broadcast() {
                    let mut one = [0u8; 8];
                    std::ptr::copy_nonoverlapping(addr, one.as_mut_ptr(), elem);
                    for lane in 0..(64 / elem) {
                        out[lane * elem..lane * elem + elem].copy_from_slice(&one[..elem]);
                    }
                } else {
                    std::ptr::copy_nonoverlapping(addr, out.as_mut_ptr(), 64);
                }
            }
        }
        k => return Err(Unsupported(format!("source operand {op} is {k:?}"))),
    }
    Ok(out)
}

fn lane_f32(b: &[u8; 64], i: usize) -> f32 {
    f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())
}
fn lane_f64(b: &[u8; 64], i: usize) -> f64 {
    f64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap())
}
fn lane_i32(b: &[u8; 64], i: usize) -> i32 {
    i32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())
}

/// Which mnemonics this build performs.
pub fn supported(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(
        m,
        Vmovups
            | Vmovupd
            | Vmovaps
            | Vmovapd
            | Vmovdqu32
            | Vmovdqu64
            | Vmovdqa32
            | Vmovdqa64
            | Vaddps
            | Vsubps
            | Vmulps
            | Vdivps
            | Vmaxps
            | Vminps
            | Vsqrtps
            | Vaddpd
            | Vsubpd
            | Vmulpd
            | Vdivpd
            | Vmaxpd
            | Vminpd
            | Vsqrtpd
            | Vfmadd132ps
            | Vfmadd213ps
            | Vfmadd231ps
            | Vfmadd132pd
            | Vfmadd213pd
            | Vfmadd231pd
            | Vfmsub132ps
            | Vfmsub213ps
            | Vfmsub231ps
            | Vpaddd
            | Vpsubd
            | Vpmulld
            | Vpandd
            | Vpord
            | Vpxord
            | Vpandnd
            | Vxorps
            | Vxorpd
            | Vandps
            | Vandpd
            | Vorps
            | Vorpd
            | Vbroadcastss
            | Vbroadcastsd
            | Vpbroadcastd
    )
}

/// Perform one AVX-512 instruction against the imaginary register file.
pub fn step(insn: &Instruction, st: &mut VState, gprs: &dyn Gprs) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();

    // Moves are their own shape: one of the two operands is memory, and
    // there is no arithmetic.
    if matches!(
        m,
        Vmovups | Vmovupd | Vmovaps | Vmovapd | Vmovdqu32 | Vmovdqu64 | Vmovdqa32 | Vmovdqa64
    ) {
        return do_move(insn, st, gprs);
    }

    let lanes = match m {
        Vaddps | Vsubps | Vmulps | Vdivps | Vmaxps | Vminps | Vsqrtps | Vfmadd132ps | Vfmadd213ps | Vfmadd231ps | Vfmsub132ps
        | Vfmsub213ps | Vfmsub231ps | Vxorps | Vandps | Vorps | Vbroadcastss => Lanes::F32,
        Vaddpd | Vsubpd | Vmulpd | Vdivpd | Vmaxpd | Vminpd | Vsqrtpd | Vfmadd132pd | Vfmadd213pd | Vfmadd231pd | Vxorpd | Vandpd
        | Vorpd | Vbroadcastsd => Lanes::F64,
        Vpaddd | Vpsubd | Vpmulld | Vpandd | Vpord | Vpxord | Vpandnd | Vpbroadcastd => Lanes::I32,
        _ => return Err(Unsupported(format!("{m:?} is not in the emulator's table"))),
    };

    let dst = zmm_index(insn.op0_register())
        .ok_or_else(|| Unsupported(format!("destination is {:?}, not a vector register", insn.op0_register())))?;
    let k = mask_index(insn.op_mask());
    let zeroing = insn.zeroing_masking();
    let width = dest_width(insn);

    // Broadcasts and square roots take one source; everything else here
    // takes two.
    let one_source = matches!(m, Vsqrtps | Vsqrtpd | Vbroadcastss | Vbroadcastsd | Vpbroadcastd);
    let (a, b) = if one_source {
        let a = source_bytes(insn, 1, st, gprs, lanes)?;
        (a, a)
    } else {
        (source_bytes(insn, 1, st, gprs, lanes)?, source_bytes(insn, 2, st, gprs, lanes)?)
    };
    // The 132/213/231 forms all read the destination as one of their
    // multiplicands, so it has to be captured before anything is written.
    let d = st.zmm[dst];

    let elem = if lanes == Lanes::F64 || lanes == Lanes::I64 { 8 } else { 4 };
    let n = width / elem;

    for i in 0..n {
        if !st.lane_enabled(k, i) {
            if zeroing {
                match lanes {
                    Lanes::F64 | Lanes::I64 => st.set_f64_lane(dst, i, 0.0),
                    _ => st.set_i32_lane(dst, i, 0),
                }
            }
            continue;
        }
        match lanes {
            Lanes::F32 => {
                let (x, y) = (lane_f32(&a, i), lane_f32(&b, i));
                let cur = f32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap());
                let v = match m {
                    Vaddps => x + y,
                    Vsubps => x - y,
                    Vmulps => x * y,
                    Vdivps => x / y,
                    // AVX-512 returns the second source when either is
                    // NaN, and when both are zero. That is not IEEE
                    // maxNum, and getting it wrong shows up only on NaN.
                    Vmaxps => {
                        if x > y {
                            x
                        } else {
                            y
                        }
                    }
                    Vminps => {
                        if x < y {
                            x
                        } else {
                            y
                        }
                    }
                    Vsqrtps => x.sqrt(),
                    Vbroadcastss => lane_f32(&a, 0),
                    // Fused: one rounding, as the hardware does.
                    Vfmadd132ps => cur.mul_add(y, x),
                    Vfmadd213ps => x.mul_add(cur, y),
                    Vfmadd231ps => x.mul_add(y, cur),
                    Vfmsub132ps => cur.mul_add(y, -x),
                    Vfmsub213ps => x.mul_add(cur, -y),
                    Vfmsub231ps => x.mul_add(y, -cur),
                    Vxorps => f32::from_bits(x.to_bits() ^ y.to_bits()),
                    Vandps => f32::from_bits(x.to_bits() & y.to_bits()),
                    Vorps => f32::from_bits(x.to_bits() | y.to_bits()),
                    _ => return Err(Unsupported(format!("{m:?} float32"))),
                };
                st.set_f32_lane(dst, i, v);
            }
            Lanes::F64 => {
                let (x, y) = (lane_f64(&a, i), lane_f64(&b, i));
                let cur = f64::from_le_bytes(d[i * 8..i * 8 + 8].try_into().unwrap());
                let v = match m {
                    Vaddpd => x + y,
                    Vsubpd => x - y,
                    Vmulpd => x * y,
                    Vdivpd => x / y,
                    Vmaxpd => {
                        if x > y {
                            x
                        } else {
                            y
                        }
                    }
                    Vminpd => {
                        if x < y {
                            x
                        } else {
                            y
                        }
                    }
                    Vsqrtpd => x.sqrt(),
                    Vbroadcastsd => lane_f64(&a, 0),
                    Vfmadd132pd => cur.mul_add(y, x),
                    Vfmadd213pd => x.mul_add(cur, y),
                    Vfmadd231pd => x.mul_add(y, cur),
                    Vxorpd => f64::from_bits(x.to_bits() ^ y.to_bits()),
                    Vandpd => f64::from_bits(x.to_bits() & y.to_bits()),
                    Vorpd => f64::from_bits(x.to_bits() | y.to_bits()),
                    _ => return Err(Unsupported(format!("{m:?} float64"))),
                };
                st.set_f64_lane(dst, i, v);
            }
            Lanes::I32 => {
                let (x, y) = (lane_i32(&a, i), lane_i32(&b, i));
                let v = match m {
                    Vpaddd => x.wrapping_add(y),
                    Vpsubd => x.wrapping_sub(y),
                    Vpmulld => x.wrapping_mul(y),
                    Vpandd => x & y,
                    Vpord => x | y,
                    Vpxord => x ^ y,
                    Vpandnd => !x & y,
                    Vpbroadcastd => lane_i32(&a, 0),
                    _ => return Err(Unsupported(format!("{m:?} int32"))),
                };
                st.set_i32_lane(dst, i, v);
            }
            Lanes::I64 => return Err(Unsupported("int64 lanes".into())),
        }
    }
    // A vector write zeroes everything above the width it wrote.
    for byte in width..64 {
        st.zmm[dst][byte] = 0;
    }
    Ok(())
}

/// Loads, stores and register-to-register moves.
fn do_move(insn: &Instruction, st: &mut VState, gprs: &dyn Gprs) -> Result<(), Unsupported> {
    let k = mask_index(insn.op_mask());
    let zeroing = insn.zeroing_masking();
    let elem = match insn.mnemonic() {
        Mnemonic::Vmovupd | Mnemonic::Vmovapd | Mnemonic::Vmovdqu64 | Mnemonic::Vmovdqa64 => 8usize,
        _ => 4usize,
    };

    match (insn.op0_kind(), insn.op1_kind()) {
        // load
        (OpKind::Register, OpKind::Memory) => {
            let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("move destination".into()))?;
            let width = dest_width(insn);
            let addr = effective_address(insn, gprs) as *const u8;
            let mut buf = [0u8; 64];
            // SAFETY: see source_bytes.
            unsafe { std::ptr::copy_nonoverlapping(addr, buf.as_mut_ptr(), width) };
            for lane in 0..(width / elem) {
                if st.lane_enabled(k, lane) {
                    st.zmm[dst][lane * elem..lane * elem + elem].copy_from_slice(&buf[lane * elem..lane * elem + elem]);
                } else if zeroing {
                    st.zmm[dst][lane * elem..lane * elem + elem].fill(0);
                }
            }
            for byte in width..64 {
                st.zmm[dst][byte] = 0;
            }
            Ok(())
        }
        // store
        (OpKind::Memory, OpKind::Register) => {
            let src = zmm_index(insn.op1_register()).ok_or_else(|| Unsupported("move source".into()))?;
            let r = insn.op1_register();
            let width = if r.is_zmm() {
                64
            } else if r.is_ymm() {
                32
            } else {
                16
            };
            let addr = effective_address(insn, gprs) as *mut u8;
            // A masked store writes only the enabled lanes and leaves the
            // rest of memory alone; there is no zeroing form of a store.
            for lane in 0..(width / elem) {
                if !st.lane_enabled(k, lane) {
                    continue;
                }
                // SAFETY: see source_bytes.
                unsafe {
                    std::ptr::copy_nonoverlapping(st.zmm[src][lane * elem..].as_ptr(), addr.add(lane * elem), elem);
                }
            }
            Ok(())
        }
        // register to register
        (OpKind::Register, OpKind::Register) => {
            let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("move destination".into()))?;
            let src = zmm_index(insn.op1_register()).ok_or_else(|| Unsupported("move source".into()))?;
            let width = dest_width(insn);
            let s = st.zmm[src];
            for lane in 0..(width / elem) {
                if st.lane_enabled(k, lane) {
                    st.zmm[dst][lane * elem..lane * elem + elem].copy_from_slice(&s[lane * elem..lane * elem + elem]);
                } else if zeroing {
                    st.zmm[dst][lane * elem..lane * elem + elem].fill(0);
                }
            }
            for byte in width..64 {
                st.zmm[dst][byte] = 0;
            }
            Ok(())
        }
        (a, b) => Err(Unsupported(format!("move with operand kinds {a:?} and {b:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_at;

    /// No memory operands in these tests, so the register view is never
    /// consulted; it exists because the signature asks for one.
    struct NoGprs;
    impl Gprs for NoGprs {
        fn get(&self, _r: Register) -> u64 {
            0
        }
    }

    fn run(bytes: &[u8], st: &mut VState) -> Result<(), Unsupported> {
        let insn = decode_at(bytes, 0x1000);
        step(&insn, st, &NoGprs)
    }

    fn splat(st: &mut VState, reg: usize, v: f32) {
        for l in 0..16 {
            st.set_f32_lane(reg, l, v);
        }
    }

    #[test]
    fn vaddps_adds_every_lane() {
        let mut st = VState::new();
        splat(&mut st, 1, 1.5);
        splat(&mut st, 2, 2.25);
        // vaddps zmm0, zmm1, zmm2
        run(&[0x62, 0xf1, 0x74, 0x48, 0x58, 0xc2], &mut st).unwrap();
        for l in 0..16 {
            assert_eq!(st.f32_lane(0, l), 3.75, "lane {l}");
        }
    }

    /// The fused multiply-add must round once, not twice.
    ///
    /// Construction: let `p` be `a * b` rounded to float32 and let
    /// `c = -p`. Rounding twice gives `p - p`, exactly zero. Rounding once
    /// gives the part of the exact product that `p` threw away, which is
    /// non-zero whenever `a * b` was inexact. So the two differ by
    /// construction rather than by luck, and an `a * b + c` implementation
    /// cannot pass this.
    #[test]
    fn fma_rounds_once_like_the_hardware() {
        let a = 1.000_000_1f32;
        let b = 1.000_000_3f32;
        let c = -(a * b);
        let fused = a.mul_add(b, c);
        let twice = a * b + c;
        assert_eq!(twice, 0.0, "rounding twice must cancel exactly");
        assert_ne!(fused, 0.0, "rounding once must keep the discarded part");

        let mut st = VState::new();
        splat(&mut st, 0, c);
        splat(&mut st, 1, a);
        splat(&mut st, 2, b);
        // vfmadd231ps zmm0, zmm1, zmm2: zmm0 = zmm1 * zmm2 + zmm0
        run(&[0x62, 0xf2, 0x75, 0x48, 0xb8, 0xc2], &mut st).unwrap();
        assert_eq!(st.f32_lane(0, 0).to_bits(), fused.to_bits());
    }

    /// A write mask leaves the disabled lanes of the destination alone.
    /// This is merging masking, which is what a bare `{k1}` means.
    #[test]
    fn a_write_mask_merges_rather_than_clearing() {
        let mut st = VState::new();
        splat(&mut st, 0, -1.0);
        splat(&mut st, 1, 1.0);
        splat(&mut st, 2, 2.0);
        st.k[1] = 0b0000_0000_0000_0101; // lanes 0 and 2 only
                                         // vaddps zmm0 {k1}, zmm1, zmm2
        run(&[0x62, 0xf1, 0x74, 0x49, 0x58, 0xc2], &mut st).unwrap();
        assert_eq!(st.f32_lane(0, 0), 3.0);
        assert_eq!(st.f32_lane(0, 1), -1.0, "lane 1 is masked off and must be untouched");
        assert_eq!(st.f32_lane(0, 2), 3.0);
        assert_eq!(st.f32_lane(0, 3), -1.0);
    }

    /// With `{z}` the disabled lanes are zeroed instead of preserved.
    #[test]
    fn zeroing_masking_clears_the_disabled_lanes() {
        let mut st = VState::new();
        splat(&mut st, 0, -1.0);
        splat(&mut st, 1, 1.0);
        splat(&mut st, 2, 2.0);
        st.k[1] = 0b0000_0000_0000_0101;
        // vaddps zmm0 {k1}{z}, zmm1, zmm2
        run(&[0x62, 0xf1, 0x74, 0xc9, 0x58, 0xc2], &mut st).unwrap();
        assert_eq!(st.f32_lane(0, 0), 3.0);
        assert_eq!(st.f32_lane(0, 1), 0.0, "lane 1 is masked off and {{z}} zeroes it");
    }

    /// The 128-bit and 256-bit EVEX forms write their own width and zero
    /// the rest of the register, which is the architectural rule for any
    /// vector write and the thing that would silently corrupt state if
    /// the width were ignored.
    #[test]
    fn narrow_forms_zero_the_upper_lanes() {
        let mut st = VState::new();
        splat(&mut st, 0, -1.0);
        splat(&mut st, 1, 1.0);
        splat(&mut st, 2, 2.0);
        // vaddps xmm0 {k1}, xmm1, xmm2 with k1 enabling everything
        st.k[1] = 0xffff;
        run(&[0x62, 0xf1, 0x74, 0x09, 0x58, 0xc2], &mut st).unwrap();
        assert_eq!(st.f32_lane(0, 0), 3.0);
        assert_eq!(st.f32_lane(0, 3), 3.0, "the four lanes it writes");
        assert_eq!(st.f32_lane(0, 4), 0.0, "everything above 128 bits is zeroed");
        assert_eq!(st.f32_lane(0, 15), 0.0);
    }

    /// Division exists here even though the card has no divide at all.
    /// The host path is not limited to what the card can do.
    #[test]
    fn divide_works_on_the_host_path() {
        let mut st = VState::new();
        splat(&mut st, 1, 1.0);
        splat(&mut st, 2, 3.0);
        // vdivps zmm0, zmm1, zmm2
        run(&[0x62, 0xf1, 0x74, 0x48, 0x5e, 0xc2], &mut st).unwrap();
        assert_eq!(st.f32_lane(0, 0).to_bits(), (1.0f32 / 3.0f32).to_bits());
    }

    /// An instruction with no emulation is an error, never a silent skip:
    /// skipping one would leave the imaginary register file disagreeing
    /// with what the program believes it computed.
    #[test]
    fn an_unknown_instruction_is_an_error() {
        // vpconflictd zmm0, zmm1 (AVX-512CD), not in the table
        let insn = decode_at(&[0x62, 0xf2, 0x7d, 0x48, 0xc4, 0xc1], 0x1000);
        assert!(!supported(insn.mnemonic()));
    }
}
