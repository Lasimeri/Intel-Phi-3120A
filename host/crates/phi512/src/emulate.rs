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

use crate::{effective_address, Cpu, VState};

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
fn source_bytes(insn: &Instruction, op: u32, st: &VState, cpu: &dyn Cpu, lanes: Lanes) -> Result<[u8; 64], Unsupported> {
    let mut out = [0u8; 64];
    match insn.op_kind(op) {
        OpKind::Register => {
            let r = insn.op_register(op);
            match zmm_index(r) {
                Some(i) => out.copy_from_slice(&st.zmm[i]),
                None if r.is_gpr() => {
                    // `vpbroadcastd zmm0, edi` and friends take a general
                    // register and splat it. The register is the element,
                    // not a vector, so every lane gets a copy.
                    let v = cpu.get(r);
                    let elem = match lanes {
                        Lanes::F64 | Lanes::I64 => 8usize,
                        _ => 4usize,
                    };
                    let bytes = v.to_le_bytes();
                    for lane in 0..(64 / elem) {
                        out[lane * elem..lane * elem + elem].copy_from_slice(&bytes[..elem]);
                    }
                }
                None => {
                    return Err(Unsupported(format!(
                        "source operand {op} is {r:?}, not a vector or general register"
                    )))
                }
            }
        }
        OpKind::Memory => {
            let addr = effective_address(insn, cpu) as *const u8;
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
/// Which mnemonics this build performs.
///
/// Deliberately a list rather than a catch-all: an instruction that is not
/// here stops the program by name, which is recoverable, whereas one that
/// is here but wrong corrupts the imaginary register file silently.
pub fn supported(m: Mnemonic) -> bool {
    use Mnemonic::*;
    if is_mask_op(m)
        || is_compare(m)
        || is_blend(m)
        || is_convert(m)
        || is_shift(m)
        || is_permute(m)
        || is_misc_int(m)
        || is_lane_move(m)
        || is_extract_insert(m)
        || is_scalar(m)
        || is_extend(m)
    {
        return true;
    }
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
            | Vpternlogd
            | Vpternlogq
            | Vpaddq
            | Vpsubq
            | Vpmullq
            | Vpandq
            | Vporq
            | Vpxorq
            | Vpandnq
            | Vpbroadcastq
    )
}

/// Perform one AVX-512 instruction against the imaginary register file.
pub fn step(insn: &Instruction, st: &mut VState, cpu: &mut dyn Cpu) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();

    // Each family below has a different shape: a different destination
    // kind, a different meaning for the write-mask, or a different lane
    // count on each side. They dispatch before the plain lane arithmetic.
    if is_extend(m) {
        return do_extend(insn, st, cpu);
    }
    if is_extract_insert(m) {
        return do_extract_insert(insn, st, cpu);
    }
    if is_scalar(m) {
        return do_scalar(insn, st, cpu);
    }
    if is_misc_int(m) {
        return do_misc_int(insn, st, cpu);
    }
    if is_lane_move(m) {
        return do_lane_move(insn, st, cpu);
    }
    if is_shift(m) {
        return do_shift(insn, st, cpu);
    }
    if is_permute(m) {
        return do_permute(insn, st, cpu);
    }
    if is_mask_op(m) {
        return do_mask(insn, st, cpu);
    }
    if is_compare(m) {
        return do_compare(insn, st, cpu);
    }
    if is_blend(m) {
        return do_blend(insn, st, cpu);
    }
    if is_convert(m) {
        return do_convert(insn, st, cpu);
    }
    if matches!(m, Vpternlogd | Vpternlogq) {
        return do_ternlog(insn, st, cpu);
    }

    // Moves are their own shape: one of the two operands is memory, and
    // there is no arithmetic.
    if matches!(
        m,
        Vmovups | Vmovupd | Vmovaps | Vmovapd | Vmovdqu32 | Vmovdqu64 | Vmovdqa32 | Vmovdqa64
    ) {
        return do_move(insn, st, cpu);
    }

    let lanes = match m {
        Vaddps | Vsubps | Vmulps | Vdivps | Vmaxps | Vminps | Vsqrtps | Vfmadd132ps | Vfmadd213ps | Vfmadd231ps | Vfmsub132ps
        | Vfmsub213ps | Vfmsub231ps | Vxorps | Vandps | Vorps | Vbroadcastss => Lanes::F32,
        Vaddpd | Vsubpd | Vmulpd | Vdivpd | Vmaxpd | Vminpd | Vsqrtpd | Vfmadd132pd | Vfmadd213pd | Vfmadd231pd | Vxorpd | Vandpd
        | Vorpd | Vbroadcastsd => Lanes::F64,
        Vpaddd | Vpsubd | Vpmulld | Vpandd | Vpord | Vpxord | Vpandnd | Vpbroadcastd => Lanes::I32,
        Vpaddq | Vpsubq | Vpmullq | Vpandq | Vporq | Vpxorq | Vpandnq | Vpbroadcastq => Lanes::I64,
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
        let a = source_bytes(insn, 1, st, cpu, lanes)?;
        (a, a)
    } else {
        (source_bytes(insn, 1, st, cpu, lanes)?, source_bytes(insn, 2, st, cpu, lanes)?)
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
            Lanes::I64 => {
                let x = i64::from_le_bytes(a[i * 8..i * 8 + 8].try_into().unwrap());
                let y = i64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
                let v = match m {
                    Vpaddq => x.wrapping_add(y),
                    Vpsubq => x.wrapping_sub(y),
                    Vpmullq => x.wrapping_mul(y),
                    Vpandq => x & y,
                    Vporq => x | y,
                    Vpxorq => x ^ y,
                    Vpandnq => !x & y,
                    Vpbroadcastq => i64::from_le_bytes(a[..8].try_into().unwrap()),
                    _ => return Err(Unsupported(format!("{m:?} int64"))),
                };
                st.zmm[dst][i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    // A vector write zeroes everything above the width it wrote.
    for byte in width..64 {
        st.zmm[dst][byte] = 0;
    }
    Ok(())
}

/// Loads, stores and register-to-register moves.
fn do_move(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
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
            let addr = effective_address(insn, cpu) as *const u8;
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
            let addr = effective_address(insn, cpu) as *mut u8;
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

// ---------------------------------------------------------------------
// The families beyond plain lane arithmetic.
//
// A program that only added vectors would need none of this. Real AVX-512
// code spends much of its time making masks, testing them, and selecting
// with them, and compilers emit `vpternlogd` constantly because it folds
// any three-input bitwise expression into one instruction.
// ---------------------------------------------------------------------

/// Width in bits of a mask instruction's operand, from its suffix.
fn mask_width(m: Mnemonic) -> Option<u32> {
    use Mnemonic::*;
    Some(match m {
        Kmovb | Kandb | Kandnb | Korb | Kxorb | Kxnorb | Knotb | Kortestb | Ktestb | Kshiftlb | Kshiftrb | Kaddb => 8,
        Kmovw | Kandw | Kandnw | Korw | Kxorw | Kxnorw | Knotw | Kortestw | Ktestw | Kshiftlw | Kshiftrw | Kaddw | Kunpckbw => 16,
        Kmovd | Kandd | Kandnd | Kord | Kxord | Kxnord | Knotd | Kortestd | Ktestd | Kshiftld | Kshiftrd | Kaddd | Kunpckwd => 32,
        Kmovq | Kandq | Kandnq | Korq | Kxorq | Kxnorq | Knotq | Kortestq | Ktestq | Kshiftlq | Kshiftrq | Kaddq | Kunpckdq => 64,
        _ => return None,
    })
}

fn width_mask(bits: u32) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

fn is_mask_op(m: Mnemonic) -> bool {
    mask_width(m).is_some()
}

/// The mask register instructions. These are AVX-512F but **VEX** encoded
/// rather than EVEX, which is why the test for "needs AVX-512" asks about
/// the required CPU feature and not about the encoding.
fn do_mask(insn: &Instruction, st: &mut VState, cpu: &mut dyn Cpu) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();
    let bits = mask_width(m).ok_or_else(|| Unsupported(format!("{m:?} is not a mask instruction")))?;
    let w = width_mask(bits);

    // Read an operand that may be a mask register, a general register, or
    // memory.
    let read = |op: u32, cpu: &dyn Cpu| -> Result<u64, Unsupported> {
        match insn.op_kind(op) {
            OpKind::Register => {
                let r = insn.op_register(op);
                if r.is_k() {
                    Ok(st.k[r as usize - Register::K0 as usize])
                } else {
                    Ok(cpu.get(r))
                }
            }
            OpKind::Memory => {
                let addr = effective_address(insn, cpu) as *const u8;
                let mut buf = [0u8; 8];
                let n = (bits as usize).div_ceil(8);
                // SAFETY: an address the program itself was about to use.
                unsafe { std::ptr::copy_nonoverlapping(addr, buf.as_mut_ptr(), n) };
                Ok(u64::from_le_bytes(buf))
            }
            OpKind::Immediate8 => Ok(u64::from(insn.immediate8())),
            k => Err(Unsupported(format!("mask operand kind {k:?}"))),
        }
    };

    if matches!(m, Kortestb | Kortestw | Kortestd | Kortestq | Ktestb | Ktestw | Ktestd | Ktestq) {
        let a = read(0, cpu)? & w;
        let b = read(1, cpu)? & w;
        // KORTEST: ZF from the OR being empty, CF from it being full.
        // KTEST: ZF from AND being empty, CF from ANDN being empty.
        let (zf, cf) = if matches!(m, Kortestb | Kortestw | Kortestd | Kortestq) {
            let t = (a | b) & w;
            (t == 0, t == w)
        } else {
            ((a & b) & w == 0, (!a & b) & w == 0)
        };
        let mut f = cpu.flags();
        // These instructions define all six arithmetic flags: the four
        // they do not use are cleared, not left stale.
        f &= !((1 << 0) | (1 << 2) | (1 << 4) | (1 << 6) | (1 << 7) | (1 << 11));
        if cf {
            f |= 1 << 0;
        }
        if zf {
            f |= 1 << 6;
        }
        cpu.set_flags(f);
        return Ok(());
    }

    let src1 = read(1, cpu)?;
    let v = match m {
        Kmovb | Kmovw | Kmovd | Kmovq => src1 & w,
        Knotb | Knotw | Knotd | Knotq => !src1 & w,
        _ => {
            let src2 = read(2, cpu)?;
            match m {
                Kandb | Kandw | Kandd | Kandq => src1 & src2,
                Kandnb | Kandnw | Kandnd | Kandnq => !src1 & src2,
                Korb | Korw | Kord | Korq => src1 | src2,
                Kxorb | Kxorw | Kxord | Kxorq => src1 ^ src2,
                Kxnorb | Kxnorw | Kxnord | Kxnorq => !(src1 ^ src2),
                Kaddb | Kaddw | Kaddd | Kaddq => src1.wrapping_add(src2),
                Kshiftlb | Kshiftlw | Kshiftld | Kshiftlq => {
                    let s = src2 & 0xff;
                    if s >= 64 {
                        0
                    } else {
                        src1 << s
                    }
                }
                Kshiftrb | Kshiftrw | Kshiftrd | Kshiftrq => {
                    let s = src2 & 0xff;
                    if s >= 64 {
                        0
                    } else {
                        (src1 & w) >> s
                    }
                }
                // The unpack forms concatenate two halves: the low half of
                // the result is src2, the high half src1.
                Kunpckbw | Kunpckwd | Kunpckdq => {
                    let half = bits / 2;
                    ((src1 & width_mask(half)) << half) | (src2 & width_mask(half))
                }
                _ => return Err(Unsupported(format!("{m:?} mask operation"))),
            }
        }
    } & w;

    match insn.op_kind(0) {
        OpKind::Register => {
            let r = insn.op0_register();
            if r.is_k() {
                st.k[r as usize - Register::K0 as usize] = v;
            } else {
                cpu.set(r, v);
            }
        }
        OpKind::Memory => {
            let addr = effective_address(insn, cpu) as *mut u8;
            let n = (bits as usize).div_ceil(8);
            // SAFETY: an address the program itself was about to use.
            unsafe { std::ptr::copy_nonoverlapping(v.to_le_bytes().as_ptr(), addr, n) };
        }
        k => return Err(Unsupported(format!("mask destination kind {k:?}"))),
    }
    Ok(())
}

/// The 32 float compare predicates of `vcmpps` and `vcmppd`.
///
/// Predicates 16 to 31 produce the same result bits as 0 to 15; they
/// differ only in whether a quiet NaN raises the invalid flag. Since this
/// emulator does not model the exception flags, they fold together, and
/// that is recorded rather than hidden: a program that compares and then
/// reads MXCSR would see a difference, and nothing else would.
// The negated forms below are the architectural definitions of these
// predicates, not an awkward way to write something positive: `NLT` is
// specified as "not less than", which is true when the operands are
// unordered. Rewriting them with `partial_cmp` would obscure the one
// thing a reader needs to check them against.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn float_predicate(imm: u8, a: f64, b: f64) -> bool {
    let unord = a.is_nan() || b.is_nan();
    match imm & 0x0f {
        0 => a == b,
        1 => a < b,
        2 => a <= b,
        3 => unord,
        4 => a != b,
        5 => !(a < b),
        6 => !(a <= b),
        7 => !unord,
        8 => a == b || unord,
        9 => !(a >= b),
        10 => !(a > b),
        11 => false,
        12 => a != b && !unord,
        13 => a >= b,
        14 => a > b,
        _ => true,
    }
}

fn is_compare(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(
        m,
        Vcmpps | Vcmppd | Vpcmpd | Vpcmpud | Vpcmpq | Vpcmpuq | Vpcmpeqd | Vpcmpgtd | Vpcmpeqq | Vpcmpgtq | Vptestmd
    )
}

/// Compares write a mask register, not a vector. This is how nearly every
/// mask in a real program comes to exist.
fn do_compare(insn: &Instruction, st: &mut VState, cpu: &mut dyn Cpu) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();
    let f64_lanes = matches!(m, Vcmppd | Vpcmpq | Vpcmpuq | Vpcmpeqq | Vpcmpgtq);
    let lanes = if f64_lanes { Lanes::F64 } else { Lanes::F32 };

    let dst = insn.op0_register();
    if !dst.is_k() {
        return Err(Unsupported(format!("compare destination is {dst:?}, not a mask register")));
    }
    let dst = dst as usize - Register::K0 as usize;
    let k = mask_index(insn.op_mask());

    let a = source_bytes(insn, 1, st, cpu, lanes)?;
    let b = source_bytes(insn, 2, st, cpu, lanes)?;
    let imm = if insn.op_count() > 3 { insn.immediate8() } else { 0 };

    let elem = if f64_lanes { 8 } else { 4 };
    let n = 64 / elem;
    let mut out = 0u64;
    for i in 0..n {
        // The write-mask of a compare does not merge: a disabled lane
        // produces a zero result bit. That is why this builds a fresh
        // value rather than editing the destination in place.
        if !st.lane_enabled(k, i) {
            continue;
        }
        let bit = match m {
            Vcmpps => float_predicate(imm, f64::from(lane_f32(&a, i)), f64::from(lane_f32(&b, i))),
            Vcmppd => float_predicate(imm, lane_f64(&a, i), lane_f64(&b, i)),
            Vpcmpeqd => lane_i32(&a, i) == lane_i32(&b, i),
            Vpcmpgtd => lane_i32(&a, i) > lane_i32(&b, i),
            Vptestmd => lane_i32(&a, i) & lane_i32(&b, i) != 0,
            Vpcmpd => int_predicate(imm, i64::from(lane_i32(&a, i)), i64::from(lane_i32(&b, i))),
            Vpcmpud => int_predicate(imm, i64::from(lane_i32(&a, i) as u32), i64::from(lane_i32(&b, i) as u32)),
            _ => return Err(Unsupported(format!("{m:?} compare"))),
        };
        if bit {
            out |= 1 << i;
        }
    }
    st.k[dst] = out;
    Ok(())
}

/// The eight integer compare predicates of `vpcmpd` and friends.
fn int_predicate(imm: u8, a: i64, b: i64) -> bool {
    match imm & 7 {
        0 => a == b,
        1 => a < b,
        2 => a <= b,
        3 => false,
        4 => a != b,
        5 => a >= b,
        6 => a > b,
        _ => true,
    }
}

/// `vpternlogd`: any bitwise function of three inputs, chosen by an
/// immediate truth table. Compilers emit this constantly, because it folds
/// what would be several boolean instructions into one.
///
/// Bit `i` of the immediate gives the result for the input combination
/// `i`, read as `(A << 2) | (B << 1) | C` where A is the destination's own
/// current value. That the destination is also an input is the thing to
/// get right: it has to be read before anything is written.
fn ternlog(a: u32, b: u32, c: u32, imm: u8) -> u32 {
    let mut r = 0u32;
    for i in 0..8u32 {
        if (imm >> i) & 1 == 1 {
            let ma = if i & 4 != 0 { a } else { !a };
            let mb = if i & 2 != 0 { b } else { !b };
            let mc = if i & 1 != 0 { c } else { !c };
            r |= ma & mb & mc;
        }
    }
    r
}

fn do_ternlog(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
    let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("ternlog destination".into()))?;
    let k = mask_index(insn.op_mask());
    let zeroing = insn.zeroing_masking();
    let width = dest_width(insn);
    let b = source_bytes(insn, 1, st, cpu, Lanes::I32)?;
    let c = source_bytes(insn, 2, st, cpu, Lanes::I32)?;
    let imm = insn.immediate8();
    let a = st.zmm[dst];
    for i in 0..(width / 4) {
        if !st.lane_enabled(k, i) {
            if zeroing {
                st.set_i32_lane(dst, i, 0);
            }
            continue;
        }
        let av = u32::from_le_bytes(a[i * 4..i * 4 + 4].try_into().unwrap());
        let bv = lane_i32(&b, i) as u32;
        let cv = lane_i32(&c, i) as u32;
        st.set_i32_lane(dst, i, ternlog(av, bv, cv, imm) as i32);
    }
    for byte in width..64 {
        st.zmm[dst][byte] = 0;
    }
    Ok(())
}

fn is_blend(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(m, Vblendmps | Vblendmpd | Vpblendmd | Vpblendmq)
}

/// Select between two sources by mask. Unlike a write-masked operation,
/// the mask here chooses which *source* supplies each lane rather than
/// whether the lane is written at all.
fn do_blend(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();
    let wide = matches!(m, Vblendmpd | Vpblendmq);
    let lanes = if wide { Lanes::F64 } else { Lanes::F32 };
    let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("blend destination".into()))?;
    let k = mask_index(insn.op_mask());
    let width = dest_width(insn);
    let a = source_bytes(insn, 1, st, cpu, lanes)?;
    let b = source_bytes(insn, 2, st, cpu, lanes)?;
    let elem = if wide { 8 } else { 4 };
    for i in 0..(width / elem) {
        let from = if st.lane_enabled(k, i) { &b } else { &a };
        st.zmm[dst][i * elem..i * elem + elem].copy_from_slice(&from[i * elem..i * elem + elem]);
    }
    for byte in width..64 {
        st.zmm[dst][byte] = 0;
    }
    Ok(())
}

fn is_convert(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(
        m,
        Vcvtdq2ps | Vcvtudq2ps | Vcvtps2dq | Vcvttps2dq | Vcvtps2pd | Vcvtpd2ps | Vcvtdq2pd
    )
}

/// Lane-width and type conversions.
fn do_convert(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();
    let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("convert destination".into()))?;
    let k = mask_index(insn.op_mask());
    let zeroing = insn.zeroing_masking();
    let src_lanes = match m {
        Vcvtdq2ps | Vcvtudq2ps | Vcvtdq2pd => Lanes::I32,
        Vcvtpd2ps => Lanes::F64,
        _ => Lanes::F32,
    };
    let a = source_bytes(insn, 1, st, cpu, src_lanes)?;
    // Widening and narrowing change how many lanes there are, so the
    // count comes from the narrower side.
    let n = match m {
        Vcvtps2pd | Vcvtdq2pd => 8,
        Vcvtpd2ps => 8,
        _ => 16,
    };
    for i in 0..n {
        if !st.lane_enabled(k, i) {
            if zeroing {
                match m {
                    Vcvtps2pd | Vcvtdq2pd => st.set_f64_lane(dst, i, 0.0),
                    _ => st.set_i32_lane(dst, i, 0),
                }
            }
            continue;
        }
        match m {
            Vcvtdq2ps => st.set_f32_lane(dst, i, lane_i32(&a, i) as f32),
            Vcvtudq2ps => st.set_f32_lane(dst, i, (lane_i32(&a, i) as u32) as f32),
            // Conversion to integer uses the current rounding mode;
            // the truncating form always rounds toward zero. Default
            // rounding is to nearest even, which `round_ties_even` is.
            Vcvtps2dq => st.set_i32_lane(dst, i, f32_to_i32(lane_f32(&a, i).round_ties_even())),
            Vcvttps2dq => st.set_i32_lane(dst, i, f32_to_i32(lane_f32(&a, i).trunc())),
            Vcvtps2pd => st.set_f64_lane(dst, i, f64::from(lane_f32(&a, i))),
            Vcvtdq2pd => st.set_f64_lane(dst, i, f64::from(lane_i32(&a, i))),
            Vcvtpd2ps => st.set_f32_lane(dst, i, lane_f64(&a, i) as f32),
            _ => return Err(Unsupported(format!("{m:?} convert"))),
        }
    }
    let written = match m {
        Vcvtpd2ps => 32,
        _ => 64,
    };
    for byte in written..64 {
        st.zmm[dst][byte] = 0;
    }
    Ok(())
}

/// x86 conversion out of range yields the "integer indefinite" value
/// rather than saturating, which is what a Rust `as` cast would do.
fn f32_to_i32(v: f32) -> i32 {
    if v.is_nan() || v > i32::MAX as f32 || v < i32::MIN as f32 {
        i32::MIN
    } else {
        v as i32
    }
}

fn is_shift(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(
        m,
        Vpslld | Vpsrld | Vpsrad | Vpsllq | Vpsrlq | Vpsraq | Vpsllvd | Vpsrlvd | Vpsravd | Vpsllvq | Vpsrlvq | Vpsravq
    )
}

/// Shifts come in two shapes that look alike and behave differently: a
/// single count applied to every lane (from an immediate or the low 64
/// bits of a vector), and a per-lane count vector. The `v` in `vpsllvd`
/// is the only thing that distinguishes them in the mnemonic.
///
/// A count at or beyond the lane width gives zero for the logical forms
/// and a full sign-fill for the arithmetic ones. Rust's `<<` panics in
/// debug and is undefined past the width, so the count is tested rather
/// than passed through.
fn do_shift(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();
    let wide = matches!(m, Vpsllq | Vpsrlq | Vpsraq | Vpsllvq | Vpsrlvq | Vpsravq);
    let per_lane = matches!(m, Vpsllvd | Vpsrlvd | Vpsravd | Vpsllvq | Vpsrlvq | Vpsravq);
    let lanes = if wide { Lanes::I64 } else { Lanes::I32 };
    let elem = if wide { 8usize } else { 4 };
    let bits = (elem * 8) as u64;

    let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("shift destination".into()))?;
    let k = mask_index(insn.op_mask());
    let zeroing = insn.zeroing_masking();
    let width = dest_width(insn);
    let a = source_bytes(insn, 1, st, cpu, lanes)?;

    // The count: an immediate, a per-lane vector, or the low 64 bits of a
    // vector used for every lane.
    let counts = if insn.op_kind(2) == OpKind::Immediate8 {
        None.or(Some(u64::from(insn.immediate8())))
    } else if per_lane {
        None
    } else {
        let c = source_bytes(insn, 2, st, cpu, Lanes::I64)?;
        Some(u64::from_le_bytes(c[..8].try_into().unwrap()))
    };
    let cvec = if per_lane {
        Some(source_bytes(insn, 2, st, cpu, lanes)?)
    } else {
        None
    };

    for i in 0..(width / elem) {
        if !st.lane_enabled(k, i) {
            if zeroing {
                if wide {
                    st.set_f64_lane(dst, i, 0.0);
                } else {
                    st.set_i32_lane(dst, i, 0);
                }
            }
            continue;
        }
        let cnt = match (&counts, &cvec) {
            (Some(c), _) => *c,
            (None, Some(v)) => {
                if wide {
                    u64::from_le_bytes(v[i * 8..i * 8 + 8].try_into().unwrap())
                } else {
                    u64::from(lane_i32(v, i) as u32)
                }
            }
            _ => 0,
        };
        if wide {
            let x = u64::from_le_bytes(a[i * 8..i * 8 + 8].try_into().unwrap());
            let v = shift64(m, x, cnt, bits);
            st.zmm[dst][i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        } else {
            let x = lane_i32(&a, i) as u32;
            let v = shift32(m, x, cnt);
            st.set_i32_lane(dst, i, v as i32);
        }
    }
    for byte in width..64 {
        st.zmm[dst][byte] = 0;
    }
    Ok(())
}

fn shift32(m: Mnemonic, x: u32, cnt: u64) -> u32 {
    use Mnemonic::*;
    match m {
        Vpslld | Vpsllvd => {
            if cnt >= 32 {
                0
            } else {
                x << cnt
            }
        }
        Vpsrld | Vpsrlvd => {
            if cnt >= 32 {
                0
            } else {
                x >> cnt
            }
        }
        // Arithmetic shift saturates to a full sign-fill rather than
        // producing zero.
        _ => {
            let c = if cnt >= 32 { 31 } else { cnt as u32 };
            ((x as i32) >> c) as u32
        }
    }
}

fn shift64(m: Mnemonic, x: u64, cnt: u64, bits: u64) -> u64 {
    use Mnemonic::*;
    match m {
        Vpsllq | Vpsllvq => {
            if cnt >= bits {
                0
            } else {
                x << cnt
            }
        }
        Vpsrlq | Vpsrlvq => {
            if cnt >= bits {
                0
            } else {
                x >> cnt
            }
        }
        _ => {
            let c = if cnt >= bits { bits - 1 } else { cnt };
            ((x as i64) >> c) as u64
        }
    }
}

fn is_permute(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(m, Vpermd | Vpermps | Vpermq | Vpermpd | Vpermt2d | Vpermt2ps | Vpermi2d | Vpermi2ps)
}

/// Cross-lane permutes. `vpermd` takes an index per lane and selects from
/// one source; the `t2` and `i2` forms select from two sources
/// concatenated, differing only in which operand is the index and which is
/// overwritten.
fn do_permute(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();
    let wide = matches!(m, Vpermq | Vpermpd);
    let elem = if wide { 8usize } else { 4 };
    let n = 64 / elem;
    let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("permute destination".into()))?;
    let k = mask_index(insn.op_mask());
    let lanes = if wide { Lanes::I64 } else { Lanes::I32 };

    let read_idx = |b: &[u8; 64], i: usize| -> usize {
        if wide {
            (u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap()) & 7) as usize
        } else {
            (lane_i32(b, i) as u32 & 15) as usize
        }
    };

    let out = match m {
        Vpermd | Vpermps | Vpermq | Vpermpd => {
            // dst = src2 permuted by indices in src1
            let idx = source_bytes(insn, 1, st, cpu, lanes)?;
            let src = source_bytes(insn, 2, st, cpu, lanes)?;
            let mut o = [0u8; 64];
            for i in 0..n {
                let j = read_idx(&idx, i);
                o[i * elem..i * elem + elem].copy_from_slice(&src[j * elem..j * elem + elem]);
            }
            o
        }
        Vpermt2d | Vpermt2ps | Vpermi2d | Vpermi2ps => {
            // Two-source: index bit above the lane count picks the second
            // table. For t2 the destination holds the first table and the
            // index is operand 1; for i2 the destination holds the index.
            let (tbl_a, idx, tbl_b) = if matches!(m, Vpermt2d | Vpermt2ps) {
                (
                    st.zmm[dst],
                    source_bytes(insn, 1, st, cpu, lanes)?,
                    source_bytes(insn, 2, st, cpu, lanes)?,
                )
            } else {
                (
                    source_bytes(insn, 1, st, cpu, lanes)?,
                    st.zmm[dst],
                    source_bytes(insn, 2, st, cpu, lanes)?,
                )
            };
            let mut o = [0u8; 64];
            for i in 0..n {
                let raw = if wide {
                    u64::from_le_bytes(idx[i * 8..i * 8 + 8].try_into().unwrap()) as usize
                } else {
                    lane_i32(&idx, i) as u32 as usize
                };
                let j = raw & (n - 1);
                let from = if raw & n != 0 { &tbl_b } else { &tbl_a };
                o[i * elem..i * elem + elem].copy_from_slice(&from[j * elem..j * elem + elem]);
            }
            o
        }
        _ => return Err(Unsupported(format!("{m:?} permute"))),
    };

    for i in 0..n {
        if st.lane_enabled(k, i) {
            st.zmm[dst][i * elem..i * elem + elem].copy_from_slice(&out[i * elem..i * elem + elem]);
        } else if insn.zeroing_masking() {
            st.zmm[dst][i * elem..i * elem + elem].fill(0);
        }
    }
    Ok(())
}

fn is_misc_int(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(
        m,
        Vpmuldq | Vpmuludq | Vpmaxsd | Vpminsd | Vpmaxud | Vpminud | Vpabsd | Vpmaxsq | Vpminsq
    )
}

/// Integer operations whose lane shape is not simply "same in, same out".
///
/// `vpmuldq` is the awkward one: it reads the *even* 32-bit elements and
/// writes 64-bit products, so sixteen input lanes become eight output
/// lanes and half the input is ignored. Treating it as a lane-for-lane
/// multiply would be wrong in a way that looks almost right.
fn do_misc_int(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();
    let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("destination".into()))?;
    let k = mask_index(insn.op_mask());
    let zeroing = insn.zeroing_masking();
    let width = dest_width(insn);

    if matches!(m, Vpmuldq | Vpmuludq) {
        let a = source_bytes(insn, 1, st, cpu, Lanes::I32)?;
        let b = source_bytes(insn, 2, st, cpu, Lanes::I32)?;
        for i in 0..(width / 8) {
            if !st.lane_enabled(k, i) {
                if zeroing {
                    st.zmm[dst][i * 8..i * 8 + 8].fill(0);
                }
                continue;
            }
            let (x, y) = (lane_i32(&a, i * 2), lane_i32(&b, i * 2));
            let v: u64 = if m == Vpmuldq {
                (i64::from(x) * i64::from(y)) as u64
            } else {
                u64::from(x as u32) * u64::from(y as u32)
            };
            st.zmm[dst][i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        for byte in width..64 {
            st.zmm[dst][byte] = 0;
        }
        return Ok(());
    }

    let wide = matches!(m, Vpmaxsq | Vpminsq);
    let lanes = if wide { Lanes::I64 } else { Lanes::I32 };
    let a = source_bytes(insn, 1, st, cpu, lanes)?;
    let b = if m == Vpabsd { a } else { source_bytes(insn, 2, st, cpu, lanes)? };
    let elem = if wide { 8 } else { 4 };
    for i in 0..(width / elem) {
        if !st.lane_enabled(k, i) {
            if zeroing {
                st.zmm[dst][i * elem..i * elem + elem].fill(0);
            }
            continue;
        }
        if wide {
            let x = i64::from_le_bytes(a[i * 8..i * 8 + 8].try_into().unwrap());
            let y = i64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
            let v = if m == Vpmaxsq { x.max(y) } else { x.min(y) };
            st.zmm[dst][i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        } else {
            let (x, y) = (lane_i32(&a, i), lane_i32(&b, i));
            let v = match m {
                Vpmaxsd => x.max(y),
                Vpminsd => x.min(y),
                Vpmaxud => ((x as u32).max(y as u32)) as i32,
                Vpminud => ((x as u32).min(y as u32)) as i32,
                Vpabsd => x.wrapping_abs(),
                _ => return Err(Unsupported(format!("{m:?}"))),
            };
            st.set_i32_lane(dst, i, v);
        }
    }
    for byte in width..64 {
        st.zmm[dst][byte] = 0;
    }
    Ok(())
}

fn is_lane_move(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(
        m,
        Vunpckhps
            | Vunpcklps
            | Vunpckhpd
            | Vunpcklpd
            | Vpunpckhdq
            | Vpunpckldq
            | Vpunpckhqdq
            | Vpunpcklqdq
            | Vshufps
            | Vshufpd
            | Vpshufd
            | Vshuff32x4
            | Vshufi32x4
            | Vshuff64x2
            | Vshufi64x2
    )
}

/// Shuffles and unpacks, which on AVX-512 work **within each 128-bit
/// group** rather than across the register. That is the detail that makes
/// a naive whole-register implementation wrong: `vunpcklps` on a 512-bit
/// register does the same thing four times, once per 128-bit lane, not
/// once across all sixteen elements.
fn do_lane_move(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();
    let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("destination".into()))?;
    let k = mask_index(insn.op_mask());
    let width = dest_width(insn);
    let wide = matches!(
        m,
        Vunpckhpd | Vunpcklpd | Vpunpckhqdq | Vpunpcklqdq | Vshufpd | Vshuff64x2 | Vshufi64x2
    );
    let lanes = if wide { Lanes::I64 } else { Lanes::I32 };

    let a = source_bytes(insn, 1, st, cpu, lanes)?;
    let b = if matches!(m, Vpshufd) {
        a
    } else {
        source_bytes(insn, 2, st, cpu, lanes)?
    };
    let imm = if insn.op_count() > 2 && insn.op_kind(insn.op_count() - 1) == OpKind::Immediate8 {
        insn.immediate8()
    } else {
        0
    };

    let mut o = [0u8; 64];
    let groups = width / 16; // 128-bit groups
    match m {
        // Interleave within each 128-bit group.
        Vunpcklps | Vpunpckldq => {
            for g in 0..groups {
                for j in 0..2 {
                    copy_elem(&mut o, g * 4 + j * 2, &a, g * 4 + j, 4);
                    copy_elem(&mut o, g * 4 + j * 2 + 1, &b, g * 4 + j, 4);
                }
            }
        }
        Vunpckhps | Vpunpckhdq => {
            for g in 0..groups {
                for j in 0..2 {
                    copy_elem(&mut o, g * 4 + j * 2, &a, g * 4 + 2 + j, 4);
                    copy_elem(&mut o, g * 4 + j * 2 + 1, &b, g * 4 + 2 + j, 4);
                }
            }
        }
        Vunpcklpd | Vpunpcklqdq => {
            for g in 0..groups {
                copy_elem(&mut o, g * 2, &a, g * 2, 8);
                copy_elem(&mut o, g * 2 + 1, &b, g * 2, 8);
            }
        }
        Vunpckhpd | Vpunpckhqdq => {
            for g in 0..groups {
                copy_elem(&mut o, g * 2, &a, g * 2 + 1, 8);
                copy_elem(&mut o, g * 2 + 1, &b, g * 2 + 1, 8);
            }
        }
        // Two bits of the immediate per output element, within the group.
        Vshufps => {
            for g in 0..groups {
                for j in 0..4 {
                    let sel = ((imm >> (j * 2)) & 3) as usize;
                    let src = if j < 2 { &a } else { &b };
                    copy_elem(&mut o, g * 4 + j, src, g * 4 + sel, 4);
                }
            }
        }
        Vpshufd => {
            for g in 0..groups {
                for j in 0..4 {
                    let sel = ((imm >> (j * 2)) & 3) as usize;
                    copy_elem(&mut o, g * 4 + j, &a, g * 4 + sel, 4);
                }
            }
        }
        Vshufpd => {
            for g in 0..groups {
                for j in 0..2 {
                    let sel = ((imm >> (g * 2 + j)) & 1) as usize;
                    let src = if j == 0 { &a } else { &b };
                    copy_elem(&mut o, g * 2 + j, src, g * 2 + sel, 8);
                }
            }
        }
        // These select whole 128-bit or 256-bit groups.
        Vshuff32x4 | Vshufi32x4 => {
            for g in 0..groups {
                let sel = ((imm >> (g * 2)) & 3) as usize;
                let src = if g < groups / 2 { &a } else { &b };
                o[g * 16..g * 16 + 16].copy_from_slice(&src[sel * 16..sel * 16 + 16]);
            }
        }
        Vshuff64x2 | Vshufi64x2 => {
            for g in 0..groups {
                let sel = ((imm >> (g * 2)) & 3) as usize;
                let src = if g < groups / 2 { &a } else { &b };
                o[g * 16..g * 16 + 16].copy_from_slice(&src[sel * 16..sel * 16 + 16]);
            }
        }
        _ => return Err(Unsupported(format!("{m:?} lane move"))),
    }

    let elem = if wide { 8 } else { 4 };
    for i in 0..(width / elem) {
        if st.lane_enabled(k, i) {
            st.zmm[dst][i * elem..i * elem + elem].copy_from_slice(&o[i * elem..i * elem + elem]);
        } else if insn.zeroing_masking() {
            st.zmm[dst][i * elem..i * elem + elem].fill(0);
        }
    }
    for byte in width..64 {
        st.zmm[dst][byte] = 0;
    }
    Ok(())
}

fn copy_elem(out: &mut [u8; 64], to: usize, from: &[u8; 64], idx: usize, elem: usize) {
    out[to * elem..to * elem + elem].copy_from_slice(&from[idx * elem..idx * elem + elem]);
}

fn is_extract_insert(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(
        m,
        Vextractf32x4
            | Vextractf64x2
            | Vextractf32x8
            | Vextractf64x4
            | Vextracti32x4
            | Vextracti64x2
            | Vextracti32x8
            | Vextracti64x4
            | Vinsertf32x4
            | Vinsertf64x2
            | Vinsertf32x8
            | Vinsertf64x4
            | Vinserti32x4
            | Vinserti64x2
            | Vinserti32x8
            | Vinserti64x4
    )
}

/// How many bytes an extract or insert moves, from the mnemonic: the
/// `x4` in `vextractf32x4` is four 32-bit elements, so 16 bytes; the `x4`
/// in `vextractf64x4` is four 64-bit elements, so 32.
fn chunk_bytes(m: Mnemonic) -> usize {
    use Mnemonic::*;
    match m {
        Vextractf32x4 | Vextracti32x4 | Vinsertf32x4 | Vinserti32x4 | Vextractf64x2 | Vextracti64x2 | Vinsertf64x2 | Vinserti64x2 => 16,
        _ => 32,
    }
}

/// Move a 128-bit or 256-bit group out of a vector, or into one. These are
/// the backbone of a horizontal reduction: fold the upper half onto the
/// lower, repeatedly, until one element is left.
fn do_extract_insert(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
    let m = insn.mnemonic();
    let bytes = chunk_bytes(m);
    let imm = usize::from(insn.immediate8());
    let inserting = format!("{m:?}").starts_with("Vinsert");

    if inserting {
        // dst = src1, with the selected group replaced by src2.
        let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("insert destination".into()))?;
        let src1 = source_bytes(insn, 1, st, cpu, Lanes::I32)?;
        let mut out = src1;
        let piece = match insn.op_kind(2) {
            OpKind::Register => {
                let r = insn.op_register(2);
                let i = zmm_index(r).ok_or_else(|| Unsupported("insert source".into()))?;
                st.zmm[i]
            }
            OpKind::Memory => {
                let addr = effective_address(insn, cpu) as *const u8;
                let mut b = [0u8; 64];
                // SAFETY: an address the program itself was about to use.
                unsafe { std::ptr::copy_nonoverlapping(addr, b.as_mut_ptr(), bytes) };
                b
            }
            k => return Err(Unsupported(format!("insert source kind {k:?}"))),
        };
        let slot = imm * bytes;
        if slot + bytes <= 64 {
            out[slot..slot + bytes].copy_from_slice(&piece[..bytes]);
        }
        st.zmm[dst] = out;
        return Ok(());
    }

    // Extract: the selected group of the source becomes the destination,
    // which is narrower than 512 bits, so everything above it is zeroed.
    let src = zmm_index(insn.op1_register()).ok_or_else(|| Unsupported("extract source".into()))?;
    let slot = imm * bytes;
    if slot + bytes > 64 {
        return Err(Unsupported("extract index out of range".into()));
    }
    match insn.op0_kind() {
        OpKind::Register => {
            let d = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("extract destination".into()))?;
            let piece: Vec<u8> = st.zmm[src][slot..slot + bytes].to_vec();
            st.zmm[d][..bytes].copy_from_slice(&piece);
            for b in bytes..64 {
                st.zmm[d][b] = 0;
            }
        }
        OpKind::Memory => {
            let addr = effective_address(insn, cpu) as *mut u8;
            // SAFETY: an address the program itself was about to use.
            unsafe { std::ptr::copy_nonoverlapping(st.zmm[src][slot..].as_ptr(), addr, bytes) };
        }
        k => return Err(Unsupported(format!("extract destination kind {k:?}"))),
    }
    Ok(())
}

fn is_scalar(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(
        m,
        Vaddss
            | Vaddsd
            | Vsubss
            | Vsubsd
            | Vmulss
            | Vmulsd
            | Vdivss
            | Vdivsd
            | Vmaxss
            | Vmaxsd
            | Vminss
            | Vminsd
            | Vsqrtss
            | Vsqrtsd
            | Vmovss
            | Vmovsd
            | Vfmadd132ss
            | Vfmadd213ss
            | Vfmadd231ss
            | Vfmadd132sd
            | Vfmadd213sd
            | Vfmadd231sd
            | Vcvtss2sd
            | Vcvtsd2ss
    )
}

/// The scalar forms operate on element 0 only and take the rest of the
/// destination from the first source. Treating them as a one-lane vector
/// operation and leaving the other lanes alone would be wrong: they are
/// copied from src1, not preserved from the destination.
fn do_scalar(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
    use Mnemonic::*;
    let m = insn.mnemonic();
    let wide = matches!(
        m,
        Vaddsd | Vsubsd | Vmulsd | Vdivsd | Vmaxsd | Vminsd | Vsqrtsd | Vmovsd | Vfmadd132sd | Vfmadd213sd | Vfmadd231sd | Vcvtss2sd
    );
    let lanes = if wide { Lanes::F64 } else { Lanes::F32 };

    // A scalar move to or from memory is just that, and has no merge.
    if matches!(m, Vmovss | Vmovsd) {
        let elem = if wide { 8 } else { 4 };
        match (insn.op0_kind(), insn.op1_kind()) {
            (OpKind::Memory, OpKind::Register) => {
                let s = zmm_index(insn.op1_register()).ok_or_else(|| Unsupported("scalar store".into()))?;
                let addr = effective_address(insn, cpu) as *mut u8;
                // SAFETY: an address the program itself was about to use.
                unsafe { std::ptr::copy_nonoverlapping(st.zmm[s].as_ptr(), addr, elem) };
                return Ok(());
            }
            (OpKind::Register, OpKind::Memory) => {
                let d = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("scalar load".into()))?;
                let addr = effective_address(insn, cpu) as *const u8;
                let mut b = [0u8; 8];
                // SAFETY: as above.
                unsafe { std::ptr::copy_nonoverlapping(addr, b.as_mut_ptr(), elem) };
                st.zmm[d] = [0u8; 64];
                st.zmm[d][..elem].copy_from_slice(&b[..elem]);
                return Ok(());
            }
            _ => {}
        }
    }

    let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("scalar destination".into()))?;
    let one_src = matches!(m, Vsqrtss | Vsqrtsd | Vcvtss2sd | Vcvtsd2ss | Vmovss | Vmovsd);
    let a = source_bytes(insn, 1, st, cpu, lanes)?;
    let b = if one_src || insn.op_count() < 3 {
        a
    } else {
        source_bytes(insn, 2, st, cpu, lanes)?
    };
    let cur = st.zmm[dst];

    // Lanes above zero come from the first source.
    let mut out = a;
    if wide {
        let x = lane_f64(&a, 0);
        let y = lane_f64(&b, 0);
        let c = f64::from_le_bytes(cur[..8].try_into().unwrap());
        let v = match m {
            Vaddsd => x + y,
            Vsubsd => x - y,
            Vmulsd => x * y,
            Vdivsd => x / y,
            Vmaxsd => {
                if x > y {
                    x
                } else {
                    y
                }
            }
            Vminsd => {
                if x < y {
                    x
                } else {
                    y
                }
            }
            Vsqrtsd => y.sqrt(),
            Vmovsd => y,
            Vcvtss2sd => f64::from(lane_f32(&b, 0)),
            Vfmadd132sd => c.mul_add(y, x),
            Vfmadd213sd => x.mul_add(c, y),
            Vfmadd231sd => x.mul_add(y, c),
            _ => return Err(Unsupported(format!("{m:?} scalar"))),
        };
        out[..8].copy_from_slice(&v.to_le_bytes());
    } else {
        let x = lane_f32(&a, 0);
        let y = lane_f32(&b, 0);
        let c = f32::from_le_bytes(cur[..4].try_into().unwrap());
        let v = match m {
            Vaddss => x + y,
            Vsubss => x - y,
            Vmulss => x * y,
            Vdivss => x / y,
            Vmaxss => {
                if x > y {
                    x
                } else {
                    y
                }
            }
            Vminss => {
                if x < y {
                    x
                } else {
                    y
                }
            }
            Vsqrtss => y.sqrt(),
            Vmovss => y,
            Vcvtsd2ss => lane_f64(&b, 0) as f32,
            Vfmadd132ss => c.mul_add(y, x),
            Vfmadd213ss => x.mul_add(c, y),
            Vfmadd231ss => x.mul_add(y, c),
            _ => return Err(Unsupported(format!("{m:?} scalar"))),
        };
        out[..4].copy_from_slice(&v.to_le_bytes());
    }
    // A scalar EVEX operation writes 128 bits and zeroes the rest.
    st.zmm[dst] = [0u8; 64];
    st.zmm[dst][..16].copy_from_slice(&out[..16]);
    Ok(())
}

/// Source and destination element widths in bytes, and whether the
/// widening is signed, for the `vpmov` family.
fn extend_shape(m: Mnemonic) -> Option<(usize, usize, bool)> {
    use Mnemonic::*;
    Some(match m {
        Vpmovsxbd => (1, 4, true),
        Vpmovzxbd => (1, 4, false),
        Vpmovsxbq => (1, 8, true),
        Vpmovzxbq => (1, 8, false),
        Vpmovsxwd => (2, 4, true),
        Vpmovzxwd => (2, 4, false),
        Vpmovsxwq => (2, 8, true),
        Vpmovzxwq => (2, 8, false),
        Vpmovsxdq => (4, 8, true),
        Vpmovzxdq => (4, 8, false),
        // The narrowing forms truncate: destination element is smaller.
        Vpmovqd => (8, 4, false),
        Vpmovqw => (8, 2, false),
        Vpmovqb => (8, 1, false),
        Vpmovdw => (4, 2, false),
        Vpmovdb => (4, 1, false),
        _ => return None,
    })
}

fn is_extend(m: Mnemonic) -> bool {
    extend_shape(m).is_some()
}

/// Widen or narrow every element. The lane *count* is the same on both
/// sides; it is the element size that changes, so the narrower side
/// occupies less of its register and the rest is zero.
fn do_extend(insn: &Instruction, st: &mut VState, cpu: &dyn Cpu) -> Result<(), Unsupported> {
    let m = insn.mnemonic();
    let (sw, dw, signed) = extend_shape(m).ok_or_else(|| Unsupported(format!("{m:?} extend")))?;
    let dst = zmm_index(insn.op0_register()).ok_or_else(|| Unsupported("extend destination".into()))?;
    let k = mask_index(insn.op_mask());
    let zeroing = insn.zeroing_masking();

    // The source may be a narrower register or memory holding only the
    // bytes the operation reads.
    let mut src = [0u8; 64];
    match insn.op1_kind() {
        OpKind::Register => {
            let i = zmm_index(insn.op1_register()).ok_or_else(|| Unsupported("extend source".into()))?;
            src = st.zmm[i];
        }
        OpKind::Memory => {
            let addr = effective_address(insn, cpu) as *const u8;
            let n = if dw > sw { 64 / dw * sw } else { 64 };
            // SAFETY: an address the program itself was about to use.
            unsafe { std::ptr::copy_nonoverlapping(addr, src.as_mut_ptr(), n.min(64)) };
        }
        kk => return Err(Unsupported(format!("extend source kind {kk:?}"))),
    }

    let widening = dw > sw;
    let n = if widening { 64 / dw } else { 64 / sw };
    let mut out = [0u8; 64];
    for i in 0..n {
        let mut v: u64 = 0;
        let bytes = &src[i * sw..i * sw + sw];
        for (j, b) in bytes.iter().enumerate() {
            v |= u64::from(*b) << (j * 8);
        }
        if signed {
            let sign_bit = 1u64 << (sw * 8 - 1);
            if v & sign_bit != 0 {
                v |= !0u64 << (sw * 8);
            }
        }
        let le = v.to_le_bytes();
        out[i * dw..i * dw + dw].copy_from_slice(&le[..dw]);
    }

    let written = if widening { 64 } else { n * dw };
    for i in 0..n {
        if st.lane_enabled(k, i) {
            st.zmm[dst][i * dw..i * dw + dw].copy_from_slice(&out[i * dw..i * dw + dw]);
        } else if zeroing {
            st.zmm[dst][i * dw..i * dw + dw].fill(0);
        }
    }
    for byte in written..64 {
        st.zmm[dst][byte] = 0;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_at;

    /// A scalar state for tests. Registers read as zero unless set, which
    /// is enough because these tests use no memory operands; the writable
    /// half exists so the mask instructions, which do write registers and
    /// flags, can be checked.
    #[derive(Default)]
    struct TestCpu {
        regs: std::collections::BTreeMap<Register, u64>,
        eflags: u64,
    }
    impl Cpu for TestCpu {
        fn get(&self, r: Register) -> u64 {
            self.regs.get(&r.full_register()).copied().unwrap_or(0)
        }
        fn set(&mut self, r: Register, v: u64) {
            self.regs.insert(r.full_register(), v);
        }
        fn flags(&self) -> u64 {
            self.eflags
        }
        fn set_flags(&mut self, f: u64) {
            self.eflags = f;
        }
    }

    fn run(bytes: &[u8], st: &mut VState) -> Result<(), Unsupported> {
        let mut cpu = TestCpu::default();
        let insn = decode_at(bytes, 0x1000);
        step(&insn, st, &mut cpu)
    }

    fn run_cpu(bytes: &[u8], st: &mut VState, cpu: &mut TestCpu) -> Result<(), Unsupported> {
        let insn = decode_at(bytes, 0x1000);
        step(&insn, st, cpu)
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

    /// The gap that made this work necessary: `kmovw` is AVX-512F but is
    /// VEX encoded, not EVEX. A detector that asks about the encoding
    /// misses the whole mask-register family, and since masks are what
    /// AVX-512 predication is built on, that is most real programs.
    #[test]
    fn mask_instructions_are_avx512_despite_being_vex_encoded() {
        let insn = decode_at(&[0xc5, 0xf8, 0x90, 0xd1], 0x1000); // kmovw k2, k1
        assert_eq!(insn.encoding(), iced_x86::EncodingKind::VEX, "it really is VEX");
        assert!(crate::is_avx512(&insn), "and it really does need AVX-512");
        assert!(supported(insn.mnemonic()));
    }

    #[test]
    fn kmov_moves_between_mask_registers_and_general_registers() {
        let mut st = VState::new();
        let mut cpu = TestCpu::default();
        st.k[1] = 0xbeef;
        // kmovw k2, k1
        run_cpu(&[0xc5, 0xf8, 0x90, 0xd1], &mut st, &mut cpu).unwrap();
        assert_eq!(st.k[2], 0xbeef);
        // kmovw eax, k2
        run_cpu(&[0xc5, 0xf8, 0x93, 0xc2], &mut st, &mut cpu).unwrap();
        assert_eq!(cpu.get(Register::RAX), 0xbeef);
    }

    /// `kortest` exists to be followed by a branch, so the flags are the
    /// whole point of it. ZF says the OR was empty, CF says it was full.
    #[test]
    fn kortest_sets_the_flags_a_branch_will_read() {
        let mut st = VState::new();
        let mut cpu = TestCpu::default();
        let zf = |c: &TestCpu| (c.flags() >> 6) & 1 == 1;
        let cf = |c: &TestCpu| c.flags() & 1 == 1;

        st.k[1] = 0;
        st.k[2] = 0;
        run_cpu(&[0xc5, 0xf8, 0x98, 0xca], &mut st, &mut cpu).unwrap(); // kortestw k1, k2
        assert!(zf(&cpu), "empty OR sets ZF");
        assert!(!cf(&cpu));

        st.k[1] = 0xffff;
        run_cpu(&[0xc5, 0xf8, 0x98, 0xca], &mut st, &mut cpu).unwrap();
        assert!(!zf(&cpu));
        assert!(cf(&cpu), "all sixteen bits set sets CF");
    }

    /// A compare writes a mask register, which is how nearly every mask in
    /// a real program comes to exist.
    #[test]
    fn a_compare_builds_a_mask() {
        let mut st = VState::new();
        for l in 0..16 {
            st.set_f32_lane(1, l, l as f32);
            st.set_f32_lane(2, l, 8.0);
        }
        // vcmpps k1, zmm1, zmm2, 1 (LT_OS)
        run(&[0x62, 0xf1, 0x74, 0x48, 0xc2, 0xca, 0x01], &mut st).unwrap();
        assert_eq!(st.k[1], 0x00ff, "lanes 0 to 7 are less than 8");
    }

    /// Comparing against NaN is false for the ordered predicates and true
    /// for the unordered ones, which is the part that separates a correct
    /// implementation from one that merely passes on ordinary numbers.
    #[test]
    fn compares_handle_nan_the_way_the_predicate_says() {
        let mut st = VState::new();
        for l in 0..16 {
            st.set_f32_lane(1, l, f32::NAN);
            st.set_f32_lane(2, l, 1.0);
        }
        run(&[0x62, 0xf1, 0x74, 0x48, 0xc2, 0xca, 0x00], &mut st).unwrap(); // EQ_OQ
        assert_eq!(st.k[1], 0, "ordered equal is false against NaN");
        run(&[0x62, 0xf1, 0x74, 0x48, 0xc2, 0xca, 0x03], &mut st).unwrap(); // UNORD_Q
        assert_eq!(st.k[1], 0xffff, "unordered is true against NaN");
        run(&[0x62, 0xf1, 0x74, 0x48, 0xc2, 0xca, 0x04], &mut st).unwrap(); // NEQ_UQ
        assert_eq!(st.k[1], 0xffff, "not-equal is true against NaN");
    }

    /// `vpternlogd` is any bitwise function of three inputs. Compilers
    /// emit it constantly. Immediate 0xca selects "B if A else C", the
    /// standard bitwise select, and 0xff is all ones.
    #[test]
    fn ternlog_computes_its_truth_table() {
        let mut st = VState::new();
        for l in 0..16 {
            st.set_i32_lane(0, l, 0x00ff_00ffu32 as i32); // A, the selector
            st.set_i32_lane(1, l, 0xaaaa_aaaau32 as i32); // B
            st.set_i32_lane(2, l, 0x5555_5555u32 as i32); // C
        }
        // vpternlogd zmm0, zmm1, zmm2, 0xca
        run(&[0x62, 0xf3, 0x75, 0x48, 0x25, 0xc2, 0xca], &mut st).unwrap();
        let want = (0x00ff_00ffu32 & 0xaaaa_aaaa) | (!0x00ff_00ffu32 & 0x5555_5555);
        assert_eq!(st.i32_lane(0, 0) as u32, want, "select B where A, else C");

        // 0xff is the constant-true table, the idiom for setting all ones
        run(&[0x62, 0xf3, 0x75, 0x48, 0x25, 0xc2, 0xff], &mut st).unwrap();
        assert_eq!(st.i32_lane(0, 0) as u32, u32::MAX);
    }

    /// A blend's mask chooses which source supplies each lane, which is a
    /// different thing from a write-mask deciding whether a lane is
    /// written at all.
    #[test]
    fn blend_selects_a_source_rather_than_suppressing_a_write() {
        let mut st = VState::new();
        for l in 0..16 {
            st.set_f32_lane(1, l, 1.0);
            st.set_f32_lane(2, l, 2.0);
        }
        st.k[1] = 0b0101;
        // vblendmps zmm0 {k1}, zmm1, zmm2
        run(&[0x62, 0xf2, 0x75, 0x49, 0x65, 0xc2], &mut st).unwrap();
        assert_eq!(st.f32_lane(0, 0), 2.0, "mask bit set takes the second source");
        assert_eq!(st.f32_lane(0, 1), 1.0, "mask bit clear takes the first");
        assert_eq!(st.f32_lane(0, 2), 2.0);
    }

    /// Converting out of range gives the integer indefinite value, not a
    /// saturated one. A Rust `as` cast saturates, so this is a place the
    /// obvious implementation is wrong.
    #[test]
    fn out_of_range_conversion_gives_the_indefinite_value() {
        let mut st = VState::new();
        for l in 0..16 {
            st.set_f32_lane(1, l, 1e30);
        }
        // vcvttps2dq zmm0, zmm1
        run(&[0x62, 0xf1, 0x7e, 0x48, 0x5b, 0xc1], &mut st).unwrap();
        assert_eq!(st.i32_lane(0, 0), i32::MIN, "not i32::MAX, which is what a saturating cast gives");
    }

    /// The exact sequence gcc emits for `(x & 0x10) ? y : -y`, which was
    /// the first thing real compiler output got wrong here.
    #[test]
    fn masked_select_sequence_from_real_compiler_output() {
        let mut st = VState::new();
        // zmm0 = 0, zmm2 = alternating 0x10 / 0, zmm3 = y
        for l in 0..16 {
            st.set_i32_lane(0, l, 0);
            st.set_i32_lane(2, l, if l % 2 == 0 { 0x10 } else { 0 });
            st.set_i32_lane(3, l, 100 + l as i32);
        }
        // vpcmpneqd k1, zmm2, zmm0   (vpcmpd with predicate 4)
        run(&[0x62, 0xf3, 0x6d, 0x48, 0x1f, 0xc8, 0x04], &mut st).unwrap();
        assert_eq!(st.k[1], 0b0101_0101_0101_0101, "k1 selects the lanes where the bit was set");

        // vpsubd zmm2, zmm0, zmm3   (zmm2 = -y)
        run(&[0x62, 0xf1, 0x7d, 0x48, 0xfa, 0xd3], &mut st).unwrap();
        assert_eq!(st.i32_lane(2, 0), -100);
        assert_eq!(st.i32_lane(2, 1), -101);

        // vmovdqa32 zmm2 {k1}, zmm3  (merge y back in where k1)
        run(&[0x62, 0xf1, 0x7d, 0x49, 0x6f, 0xd3], &mut st).unwrap();
        assert_eq!(st.i32_lane(2, 0), 100, "enabled lane takes y");
        assert_eq!(st.i32_lane(2, 1), -101, "disabled lane keeps -y");
        assert_eq!(st.i32_lane(2, 2), 102);
        assert_eq!(st.i32_lane(2, 3), -103);
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
