//! The part that makes it invisible: catch the processor's refusal, do the
//! work, and put the program back where it was.
//!
//! Installed from `.init_array`, so `LD_PRELOAD` is enough and the program
//! needs no cooperation of any kind.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use iced_x86::Register;

use crate::{decode_at, emulate, is_avx512, Cpu, VState};

// glibc's ordering of `uc_mcontext.gregs` on x86-64.
const REG_R8: usize = 0;
const REG_R9: usize = 1;
const REG_R10: usize = 2;
const REG_R11: usize = 3;
const REG_R12: usize = 4;
const REG_R13: usize = 5;
const REG_R14: usize = 6;
const REG_R15: usize = 7;
const REG_RDI: usize = 8;
const REG_RSI: usize = 9;
const REG_RBP: usize = 10;
const REG_RBX: usize = 11;
const REG_RDX: usize = 12;
const REG_RAX: usize = 13;
const REG_RCX: usize = 14;
const REG_RSP: usize = 15;
const REG_RIP: usize = 16;
const REG_EFL: usize = 17;

static EMULATED: AtomicU64 = AtomicU64::new(0);
static VERBOSE: AtomicBool = AtomicBool::new(false);
static TRACE: AtomicBool = AtomicBool::new(false);
static PREV_SIGTRAP: AtomicUsize = AtomicUsize::new(0);
static PATCHING_ON: AtomicBool = AtomicBool::new(true);

/// The program's scalar registers, read and written straight in the signal
/// frame. Writing here is how a result reaches the program: when the
/// handler returns, the kernel restores registers from this frame, so a
/// value stored into `gregs` is in the register when the program resumes.
struct Frame(*mut libc::ucontext_t);

impl Frame {
    /// Map a register name to its slot. The 32-bit and 16-bit names
    /// address the same machine register; the width matters for how much
    /// of it an instruction uses, not for which slot holds it.
    fn slot(r: Register) -> Option<usize> {
        Some(match r.full_register() {
            Register::RAX => REG_RAX,
            Register::RCX => REG_RCX,
            Register::RDX => REG_RDX,
            Register::RBX => REG_RBX,
            Register::RSP => REG_RSP,
            Register::RBP => REG_RBP,
            Register::RSI => REG_RSI,
            Register::RDI => REG_RDI,
            Register::R8 => REG_R8,
            Register::R9 => REG_R9,
            Register::R10 => REG_R10,
            Register::R11 => REG_R11,
            Register::R12 => REG_R12,
            Register::R13 => REG_R13,
            Register::R14 => REG_R14,
            Register::R15 => REG_R15,
            _ => return None,
        })
    }
}

impl Cpu for Frame {
    fn get(&self, r: Register) -> u64 {
        match Frame::slot(r) {
            // SAFETY: the kernel handed us this frame for this signal.
            Some(i) => unsafe { (*self.0).uc_mcontext.gregs[i] as u64 },
            None => 0,
        }
    }

    fn set(&mut self, r: Register, v: u64) {
        if let Some(i) = Frame::slot(r) {
            // Writing a 32-bit register zeroes the upper half, which is
            // the x86-64 rule and matters for `kmov eax, k1`.
            let v = if r.size() == 4 { v & 0xffff_ffff } else { v };
            // SAFETY: as above.
            unsafe { (*self.0).uc_mcontext.gregs[i] = v as i64 };
        }
    }

    fn flags(&self) -> u64 {
        // SAFETY: as above.
        unsafe { (*self.0).uc_mcontext.gregs[REG_EFL] as u64 }
    }

    fn set_flags(&mut self, f: u64) {
        // SAFETY: as above.
        unsafe { (*self.0).uc_mcontext.gregs[REG_EFL] = f as i64 };
    }
}

/// Render bytes as hex into a fixed buffer. Allocation-free, because this
/// is used from the signal handler.
fn hex<'a>(bytes: &[u8], buf: &'a mut [u8; 64]) -> &'a str {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut n = 0;
    for &b in bytes {
        if n + 3 > buf.len() {
            break;
        }
        buf[n] = D[(b >> 4) as usize];
        buf[n + 1] = D[(b & 15) as usize];
        buf[n + 2] = b' ';
        n += 3;
    }
    std::str::from_utf8(&buf[..n]).unwrap_or("?")
}

/// Write a message without allocating or locking, because this is a signal
/// handler and `println!` is neither of those things.
fn say(parts: &[&str]) {
    let mut buf = [0u8; 512];
    let mut n = 0;
    for p in parts {
        for &b in p.as_bytes() {
            if n < buf.len() {
                buf[n] = b;
                n += 1;
            }
        }
    }
    // SAFETY: write to stderr of a byte buffer we own.
    unsafe { libc::write(2, buf.as_ptr() as *const libc::c_void, n) };
}

/// Format a small number into a fixed buffer, signal-handler safe.
fn num(mut v: u64, buf: &mut [u8; 24]) -> &str {
    if v == 0 {
        buf[0] = b'0';
        return std::str::from_utf8(&buf[..1]).unwrap_or("0");
    }
    let mut tmp = [0u8; 24];
    let mut i = 0;
    while v > 0 {
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    for j in 0..i {
        buf[j] = tmp[i - 1 - j];
    }
    std::str::from_utf8(&buf[..i]).unwrap_or("?")
}

extern "C" fn on_sigill(_sig: i32, _info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    let uc = ctx as *mut libc::ucontext_t;
    // SAFETY: the kernel handed us this frame.
    let rip = unsafe { (*uc).uc_mcontext.gregs[REG_RIP] } as u64;

    // A fault can outlive the instruction that caused it. Between the
    // processor raising this signal and the handler running, another
    // thread may have rewritten this very site, so the bytes here are now
    // a jump rather than the AVX-512 instruction that faulted. Decoding
    // them would find a perfectly legal `jmp`, conclude the fault was not
    // ours, and kill the program.
    //
    // Returning without moving the instruction pointer re-executes the
    // site, which now jumps to the stub and does the work properly.
    if let Some(index) = crate::patch::site_at(rip) {
        crate::patch::await_ready(index);
        crate::patch::RACED.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // The longest x86-64 instruction is 15 bytes. Reading them is safe:
    // the processor just fetched from here.
    let bytes = unsafe { std::slice::from_raw_parts(rip as *const u8, 15) };
    let insn = decode_at(bytes, rip);

    if !is_avx512(&insn) {
        // Check again before giving up. The first check happened before
        // these bytes were read, and another thread can rewrite the site
        // in between: the fault was raised for an AVX-512 instruction
        // that no longer exists, and what was read is the jump that
        // replaced it. Giving up here would kill a program that is
        // perfectly healthy.
        if let Some(index) = crate::patch::site_at(rip) {
            crate::patch::await_ready(index);
            crate::patch::RACED.fetch_add(1, Ordering::Relaxed);
            return;
        }
        // Genuinely not ours. Restore the default action and return, so
        // the process dies the way it would have without this library.
        if VERBOSE.load(Ordering::Relaxed) {
            let mut hb = [0u8; 64];
            let mut nb = [0u8; 24];
            let mut cb = [0u8; 24];
            let mut sb = [0u8; 24];
            say(&[
                "phi512: illegal instruction that is not AVX-512 at ",
                num(rip, &mut nb),
                ": ",
                hex(&bytes[..8], &mut hb),
                " sites=",
                num(crate::patch::site_count() as u64, &mut cb),
                " nearest=",
                num(crate::patch::nearest_site(rip), &mut sb),
                "\n",
            ]);
        }
        unsafe {
            libc::signal(libc::SIGILL, libc::SIG_DFL);
        }
        return;
    }

    if !emulate::supported(insn.mnemonic()) {
        let mut b = [0u8; 24];
        say(&[
            "phi512: no emulation for ",
            &format!("{:?}", insn.mnemonic()),
            " at instruction ",
            num(EMULATED.load(Ordering::Relaxed), &mut b),
            "\nphi512: this is a gap in the emulator, not a fault in the program.\n",
        ]);
        unsafe {
            libc::signal(libc::SIGILL, libc::SIG_DFL);
        }
        return;
    }

    // PHI512_TRACE prints every instruction as it is performed. The last
    // line before a crash names the instruction that caused it, which is
    // the only practical way to debug a fault inside a fault handler.
    if TRACE.load(Ordering::Relaxed) {
        let mut hb = [0u8; 64];
        let mut nb = [0u8; 24];
        say(&[
            "phi512: ",
            hex(&bytes[..insn.len().min(10)], &mut hb),
            " n=",
            num(EMULATED.load(Ordering::Relaxed), &mut nb),
            "\n",
        ]);
    }
    // The low 256 bits of zmm0 to zmm15 are real hardware that AVX2
    // instructions use without faulting, so they are read fresh from the
    // frame rather than remembered.
    let mut frame = Frame(uc);
    let result = crate::state::tls::with(|st| {
        pull_live_registers(uc, st);
        let r = emulate::step(&insn, st, &mut frame);
        if r.is_ok() {
            push_live_registers(uc, st);
            st.note_write();
        }
        r
    });

    if let Err(e) = result {
        say(&["phi512: ", &e.0, "\n"]);
        unsafe {
            libc::signal(libc::SIGILL, libc::SIG_DFL);
        }
        return;
    }

    EMULATED.fetch_add(1, Ordering::Relaxed);

    // Now that the instruction has been performed once, rewrite the site
    // so it never faults again. This happens after the emulation, not
    // instead of it: the program is mid-instruction and still needs this
    // one done. Failure is not an error, it only means the site keeps
    // faulting.
    if PATCHING_ON.load(Ordering::Relaxed) {
        crate::patch::try_patch(rip, insn.len(), bytes);
    }

    // Step over the instruction we just performed.
    unsafe {
        (*uc).uc_mcontext.gregs[REG_RIP] = insn.next_ip() as i64;
    }
}

/// Someone executed the breakpoint that sits in a site for the few stores
/// it takes to rewrite it.
///
/// This is not an error: it means another thread reached the instruction
/// while it was being replaced. Wait for the rewrite to finish, put the
/// instruction pointer back to the start of the site, and let it run the
/// jump that is now there.
///
/// A breakpoint anywhere else belongs to somebody else, most likely a
/// debugger, and is passed on untouched.
extern "C" fn on_sigtrap(sig: i32, info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    let uc = ctx as *mut libc::ucontext_t;
    // SAFETY: the kernel handed us this frame.
    let after = unsafe { (*uc).uc_mcontext.gregs[REG_RIP] } as u64;
    // int3 is one byte, and the reported address is the one after it.
    let site_addr = after.wrapping_sub(1);

    crate::patch::TRAPS.fetch_add(1, Ordering::Relaxed);
    if let Some(index) = crate::patch::site_at(site_addr) {
        crate::patch::await_ready(index);
        // SAFETY: as above. Re-executing the site now runs the jump.
        unsafe { (*uc).uc_mcontext.gregs[REG_RIP] = site_addr as i64 };
        return;
    }

    crate::patch::TRAPS_FOREIGN.fetch_add(1, Ordering::Relaxed);
    // Not ours. Hand it to whoever had SIGTRAP before, or to the default.
    let prev = PREV_SIGTRAP.load(Ordering::Relaxed);
    if prev != 0 && prev != libc::SIG_DFL && prev != libc::SIG_IGN {
        // SAFETY: the value came from a previous sigaction, so it is a
        // handler of this shape.
        let f: extern "C" fn(i32, *mut libc::siginfo_t, *mut libc::c_void) = unsafe { std::mem::transmute(prev) };
        f(sig, info, ctx);
        return;
    }
    // SAFETY: restoring the default action for a signal we installed.
    unsafe {
        libc::signal(libc::SIGTRAP, libc::SIG_DFL);
    }
}
extern "C" fn report() {
    if !VERBOSE.load(Ordering::Relaxed) {
        return;
    }
    let mut a = [0u8; 24];
    let mut b = [0u8; 24];
    let mut c = [0u8; 24];
    let mut d = [0u8; 24];
    let mut e = [0u8; 24];
    say(&[
        "phi512: performed ",
        num(EMULATED.load(Ordering::Relaxed), &mut a),
        " AVX-512 instructions, rewrote ",
        num(crate::patch::PATCHED.load(Ordering::Relaxed), &mut b),
        " sites (",
        num(crate::patch::TOO_SHORT.load(Ordering::Relaxed), &mut c),
        " too short to rewrite), ",
        num(crate::patch::TRAPS.load(Ordering::Relaxed), &mut d),
        " breakpoints (",
        num(crate::patch::TRAPS_FOREIGN.load(Ordering::Relaxed), &mut e),
        " unrecognised)\n",
    ]);
}

/// Does this processor already have AVX-512? If so there is nothing to do
/// and the handler is not installed: catching SIGILL on a machine where
/// these instructions work would only add risk.
fn host_has_avx512() -> bool {
    // SAFETY: CPUID leaf 7 is architectural on anything running this code.
    {
        if core::arch::x86_64::__cpuid(0).eax < 7 {
            return false;
        }
        core::arch::x86_64::__cpuid_count(7, 0).ebx & (1 << 16) != 0
    }
}

/// Installed by the loader before the program's own `main` runs.
///
/// This may be loaded into **every process on the system** through
/// `/etc/ld.so.preload`, including setuid binaries like `sudo`, so the
/// contract here is strict: if anything is not as expected, return
/// quietly and leave the process exactly as it would have been. It must
/// never abort, never print unless asked, and never prevent a program
/// from starting. A library that can brick the machine it is installed on
/// is not worth the instructions it emulates.
extern "C" fn init() {
    // An explicit off switch that needs no root and no file edit. If this
    // library ever makes a machine unbootable, this is the thing that can
    // be set in a rescue shell.
    if std::env::var_os("PHI512_DISABLE").is_some() {
        return;
    }
    if host_has_avx512() {
        return;
    }

    VERBOSE.store(std::env::var_os("PHI512_VERBOSE").is_some(), Ordering::Relaxed);
    TRACE.store(std::env::var_os("PHI512_TRACE").is_some(), Ordering::Relaxed);
    PATCHING_ON.store(std::env::var_os("PHI512_NOPATCH").is_none(), Ordering::Relaxed);
    YMM_OFFSET.store(probe_ymm_offset(), Ordering::Relaxed);

    // SAFETY: standard sigaction installation. A failure here is not
    // fatal: without the handler the process behaves exactly as it would
    // without this library loaded.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_sigill as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART | libc::SA_ONSTACK;
        libc::sigemptyset(&mut sa.sa_mask);
        if libc::sigaction(libc::SIGILL, &sa, std::ptr::null_mut()) != 0 {
            return;
        }

        // The breakpoint handler only matters while a site is being
        // rewritten, but it has to be in place before the first rewrite,
        // and whatever had SIGTRAP before is kept so a debugger still
        // works.
        if PATCHING_ON.load(Ordering::Relaxed) {
            let mut old: libc::sigaction = std::mem::zeroed();
            let mut t: libc::sigaction = std::mem::zeroed();
            t.sa_sigaction = on_sigtrap as *const () as usize;
            t.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART | libc::SA_ONSTACK;
            libc::sigemptyset(&mut t.sa_mask);
            if libc::sigaction(libc::SIGTRAP, &t, &mut old) == 0 {
                PREV_SIGTRAP.store(old.sa_sigaction, Ordering::Relaxed);
            } else {
                // Without it a rewrite could lose a thread to an
                // unhandled breakpoint, so do not rewrite anything.
                PATCHING_ON.store(false, Ordering::Relaxed);
            }
        }
        libc::atexit(report);
    }

    if VERBOSE.load(Ordering::Relaxed) {
        say(&["phi512: AVX-512 will be performed in software on this host\n"]);
    }
}

#[used]
#[link_section = ".init_array"]
static INIT_ARRAY: extern "C" fn() = init;

// ---------------------------------------------------------------------
// Keeping the imaginary registers and the real ones in agreement.
//
// The low 128 bits of every zmm register are an xmm register, and the low
// 256 bits are a ymm register, and both of those are **real hardware on
// this host**. An AVX or AVX2 instruction touching them executes natively
// and never faults, so nothing here ever sees it.
//
// A program mixes the two constantly. A horizontal reduction is the
// ordinary case:
//
//     vextracti64x4 ymm1, zmm2, 1     AVX-512: faults, handled here
//     vpaddd        ymm0, ymm0, ymm1  AVX2: runs on the real registers
//
// If the emulator kept its own copy of ymm1, the second instruction would
// read the hardware's ymm1, which the first never wrote, and the answer
// would be silently wrong. So the low 256 bits are not imaginary at all:
// they are read out of the signal frame before each instruction and
// written back after, and only bits 256 and above of zmm0 to zmm15, plus
// the whole of zmm16 to zmm31, are storage this library owns.
//
// The registers live in the signal frame's XSAVE area: the xmm halves in
// the legacy FXSAVE region at offset 160, and the ymm upper halves in the
// YMM_Hi128 state component, whose offset the processor reports through
// CPUID leaf 0x0D sub-leaf 2.

const FXSAVE_XMM_OFFSET: usize = 160;
const XSTATE_BV_OFFSET: usize = 512;
const XFEATURE_YMM: u64 = 1 << 2;

/// Offset of the YMM_Hi128 component inside the XSAVE area, as the
/// processor reports it. Zero means the processor does not have the
/// component, in which case there are no ymm upper halves to sync.
static YMM_OFFSET: AtomicU64 = AtomicU64::new(0);

fn probe_ymm_offset() -> u64 {
    // SAFETY: CPUID leaf 0x0D is architectural and this host supports AVX,
    // which was checked by the caller.
    {
        let max = core::arch::x86_64::__cpuid(0).eax;
        if max < 0x0d {
            return 0;
        }
        let leaf = core::arch::x86_64::__cpuid_count(0x0d, 2);
        u64::from(leaf.ebx)
    }
}

/// Copy the real xmm and ymm registers out of the signal frame into the
/// low 32 bytes of the emulator's view, and discard any upper half that
/// a VEX instruction would have zeroed. See `VState::upper_is_stale`.
fn pull_live_registers(uc: *mut libc::ucontext_t, st: &mut VState) {
    // SAFETY: the kernel handed us this frame, and fpregs points at the
    // save area it wrote for this signal.
    unsafe {
        let fp = (*uc).uc_mcontext.fpregs as *const u8;
        if fp.is_null() {
            return;
        }
        let off = YMM_OFFSET.load(Ordering::Relaxed) as usize;
        let have_ymm = off != 0 && {
            let bv = std::ptr::read_unaligned(fp.add(XSTATE_BV_OFFSET) as *const u64);
            bv & XFEATURE_YMM != 0
        };
        for i in 0..16 {
            let mut live = [0u8; 32];
            std::ptr::copy_nonoverlapping(fp.add(FXSAVE_XMM_OFFSET + i * 16), live.as_mut_ptr(), 16);
            if have_ymm {
                std::ptr::copy_nonoverlapping(fp.add(off + i * 16), live[16..].as_mut_ptr(), 16);
            }
            // If the program wrote this register with an instruction this
            // library never saw, that instruction was VEX or SSE encoded,
            // and a VEX write zeroes everything above 128 bits on real
            // AVX-512 hardware. Bits 256 and up are this library's, so it
            // has to apply that rule itself.
            if st.upper_is_stale(i, &live) {
                st.zmm[i][32..].fill(0);
            }
            st.zmm[i][..32].copy_from_slice(&live);
        }
    }
}

/// Write the low 32 bytes back, so the program resumes with whatever the
/// emulated instruction produced.
fn push_live_registers(uc: *mut libc::ucontext_t, st: &VState) {
    // SAFETY: as above.
    unsafe {
        let fp = (*uc).uc_mcontext.fpregs as *mut u8;
        if fp.is_null() {
            return;
        }
        for i in 0..16 {
            std::ptr::copy_nonoverlapping(st.zmm[i].as_ptr(), fp.add(FXSAVE_XMM_OFFSET + i * 16), 16);
        }
        let off = YMM_OFFSET.load(Ordering::Relaxed) as usize;
        if off == 0 {
            return;
        }
        // Announce the component as live, or the kernel will restore
        // zeros over what was just written.
        let bv = std::ptr::read_unaligned(fp.add(XSTATE_BV_OFFSET) as *const u64);
        std::ptr::write_unaligned(fp.add(XSTATE_BV_OFFSET) as *mut u64, bv | XFEATURE_YMM);
        for i in 0..16 {
            std::ptr::copy_nonoverlapping(st.zmm[i][16..].as_ptr(), fp.add(off + i * 16), 16);
        }
    }
}
