//! The part that makes it invisible: catch the processor's refusal, do the
//! work, and put the program back where it was.
//!
//! Installed from `.init_array`, so `LD_PRELOAD` is enough and the program
//! needs no cooperation of any kind.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use iced_x86::Register;

use crate::{decode_at, emulate, is_avx512, Gprs, VState};

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

static EMULATED: AtomicU64 = AtomicU64::new(0);
static VERBOSE: AtomicBool = AtomicBool::new(false);

// The imaginary register file, one per thread. `const` initialisation
// means no allocation happens on first touch, which matters because the
// first touch is inside a signal handler.
thread_local! {
    static STATE: std::cell::UnsafeCell<VState> = const { std::cell::UnsafeCell::new(VState::new()) };
}

/// The program's registers, read straight out of the signal frame.
struct Frame(*mut libc::ucontext_t);

impl Gprs for Frame {
    fn get(&self, r: Register) -> u64 {
        // The 32-bit and 16-bit names address the same machine register;
        // the width only matters for how much of it the instruction uses,
        // and an address computation uses all of it.
        let idx = match r.full_register() {
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
            _ => return 0,
        };
        // SAFETY: the kernel handed us this frame for this signal.
        unsafe { (*self.0).uc_mcontext.gregs[idx] as u64 }
    }
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

    // The longest x86-64 instruction is 15 bytes. Reading them is safe:
    // the processor just fetched from here.
    let bytes = unsafe { std::slice::from_raw_parts(rip as *const u8, 15) };
    let insn = decode_at(bytes, rip);

    if !is_avx512(&insn) {
        // Not ours. Restore the default action and return, so the process
        // dies the way it would have without this library loaded.
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

    let frame = Frame(uc);
    let result = STATE.with(|s| {
        // SAFETY: one thread, one state, and a signal handler on that
        // thread cannot run concurrently with itself.
        let st = unsafe { &mut *s.get() };
        emulate::step(&insn, st, &frame)
    });

    if let Err(e) = result {
        say(&["phi512: ", &e.0, "\n"]);
        unsafe {
            libc::signal(libc::SIGILL, libc::SIG_DFL);
        }
        return;
    }

    EMULATED.fetch_add(1, Ordering::Relaxed);
    // Step over the instruction we just performed.
    unsafe {
        (*uc).uc_mcontext.gregs[REG_RIP] = insn.next_ip() as i64;
    }
}

extern "C" fn report() {
    if VERBOSE.load(Ordering::Relaxed) {
        let mut b = [0u8; 24];
        say(&[
            "phi512: performed ",
            num(EMULATED.load(Ordering::Relaxed), &mut b),
            " AVX-512 instructions\n",
        ]);
    }
}

/// Installed by the loader before the program's own `main` runs.
extern "C" fn init() {
    VERBOSE.store(std::env::var_os("PHI512_VERBOSE").is_some(), Ordering::Relaxed);

    // SAFETY: standard sigaction installation.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_sigill as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART | libc::SA_ONSTACK;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGILL, &sa, std::ptr::null_mut());
        libc::atexit(report);
    }

    if VERBOSE.load(Ordering::Relaxed) {
        say(&["phi512: AVX-512 will be performed in software on this host\n"]);
    }
}

#[used]
#[link_section = ".init_array"]
static INIT_ARRAY: extern "C" fn() = init;
