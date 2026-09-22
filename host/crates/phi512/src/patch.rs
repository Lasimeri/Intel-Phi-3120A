//! Rewriting a faulting site so it never faults again.
//!
//! A fault costs about 1909 ns on this host and the arithmetic behind it
//! costs under 200, so nine tenths of the price of emulating an AVX-512
//! instruction is the processor telling us it happened. Paying that once
//! per site instead of once per execution is the whole of this module.
//!
//! # Why it fits
//!
//! An EVEX instruction is at least six bytes: the `62H` prefix and three
//! payload bytes, an opcode, and a ModRM byte. A near jump is five. So a
//! site can always be overwritten in place with a jump to code that does
//! the same thing, with a byte to spare.
//!
//! The exception is the mask register family. `kmovw k2, k1` is AVX-512F
//! but **VEX** encoded and four bytes long, and no five-byte jump fits in
//! four bytes. Those sites keep faulting, and are counted separately so
//! the cost is visible rather than mysterious.
//!
//! # Shape of a patched site
//!
//! ```text
//!   original:  62 f1 74 48 58 c2        vaddps zmm0, zmm1, zmm2
//!   patched:   e9 <rel32> 90            jmp stub
//!
//!   stub:      lea  rsp, [rsp-128]      step over the red zone
//!              call trampoline
//!              lea  rsp, [rsp+128]
//!              jmp  <after the original instruction>
//! ```
//!
//! The stub takes no arguments. The trampoline works out which site it
//! was called from by looking at the return address, which is why the
//! stubs are a fixed size in one arena: the index is a division.
//! Passing an index in a register instead would have clobbered a register
//! before anything had saved it.
//!
//! # Why the frame goes on the stack
//!
//! The trampoline spills the program's registers to its own stack and
//! passes a pointer to them. Two threads in the same stub have different
//! stacks, so they have different frames and cannot interfere. Nothing
//! here needs a lock on the hot path.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};

use crate::frame::PatchFrame;

/// Bytes reserved for each site's stub. The stub itself is 23; the round
/// number makes the return-address-to-index calculation a shift.
pub const STUB_SIZE: usize = 32;

/// Offset of the instruction after the `call` inside a stub, which is
/// what the return address points at.
const STUB_RET_OFFSET: usize = 10;

/// How many sites can be patched. Each costs 32 bytes of stub and one
/// table entry; this is generous for any real program and bounded so the
/// table can be a static array reachable from a signal handler without
/// allocating.
pub const MAX_SITES: usize = 8192;

/// A site is published only after its stub is complete, so a reader that
/// sees `Ready` can trust everything about it. `Failed` is a site that
/// was published and then could not be rewritten (the text could not be
/// made writable); its instruction is still the original and still
/// faults, and nothing may wait on it.
const SITE_EMPTY: u8 = 0;
const SITE_PATCHING: u8 = 1;
const SITE_READY: u8 = 2;
const SITE_FAILED: u8 = 3;

/// One rewritten instruction.
#[repr(C)]
pub struct Site {
    /// Where the instruction was.
    pub rip: AtomicU64,
    /// Its length, so the stub knows where to resume.
    pub len: AtomicU32,
    /// The instruction's own bytes, kept because the original is gone
    /// once the jump is written over it.
    pub bytes: [AtomicU8; 16],
    pub state: AtomicU8,
    /// Which of zmm0 to zmm15 this instruction names, so only those are
    /// moved in and out on each execution.
    pub regs: AtomicU32,
}

impl Site {
    #[allow(clippy::declare_interior_mutable_const)]
    const NEW: Site = Site {
        rip: AtomicU64::new(0),
        len: AtomicU32::new(0),
        // 16 separate atomics rather than one array, so the whole table
        // can be a `static` with no initialiser to run at load time.
        bytes: [const { AtomicU8::new(0) }; 16],
        state: AtomicU8::new(SITE_EMPTY),
        regs: AtomicU32::new(0),
    };
}

#[allow(clippy::declare_interior_mutable_const)]
static SITES: [Site; MAX_SITES] = [const { Site::NEW }; MAX_SITES];
static SITE_COUNT: AtomicU32 = AtomicU32::new(0);

/// Base of the executable arena: trampoline first, then the stubs.
static ARENA: AtomicU64 = AtomicU64::new(0);
/// Where the stubs start inside the arena.
static STUBS: AtomicU64 = AtomicU64::new(0);
/// Set once the arena could not be created, so it is not retried forever.
static ARENA_FAILED: AtomicBool = AtomicBool::new(false);

/// Sites that could not be patched because the instruction was shorter
/// than a jump. Reported, not hidden.
pub static TOO_SHORT: AtomicU64 = AtomicU64::new(0);
/// Sites successfully rewritten.
pub static PATCHED: AtomicU64 = AtomicU64::new(0);
/// Breakpoints taken in the rewrite window, and ones not recognised.
pub static TRAPS: AtomicU64 = AtomicU64::new(0);
/// Faults that arrived after their site had already been rewritten.
pub static RACED: AtomicU64 = AtomicU64::new(0);
pub static TRAPS_FOREIGN: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------
// Emitting machine code
// ---------------------------------------------------------------------

/// Machine code being assembled, in a fixed buffer.
///
/// No heap: the stub builder runs inside the `SIGILL` handler, and the
/// thread that faulted may have been inside the allocator holding its
/// lock. The trampoline is under 400 bytes and a stub is 23.
pub struct Code {
    buf: [u8; 512],
    len: usize,
}

impl Code {
    const fn new() -> Code {
        Code { buf: [0; 512], len: 0 }
    }

    fn push(&mut self, b: u8) {
        self.buf[self.len] = b;
        self.len += 1;
    }

    fn extend_from_slice(&mut self, s: &[u8]) {
        self.buf[self.len..self.len + s.len()].copy_from_slice(s);
        self.len += s.len();
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl std::ops::Deref for Code {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

/// `vmovups` to or from `[rsp + disp]`, with a 32-bit displacement so the
/// encoding is the same length for every register and offset.
///
/// The two-byte VEX form is used: `C5` then one byte carrying an inverted
/// register-extension bit, the unused `vvvv`, the 256-bit length bit and
/// no legacy prefix. That is `0xFC` for `ymm0` to `ymm7` and `0x7C` for
/// `ymm8` to `ymm15`.
fn emit_vmovups(out: &mut Code, reg: u8, disp: i32, store: bool) {
    out.push(0xc5);
    out.push(if reg < 8 { 0xfc } else { 0x7c });
    out.push(if store { 0x11 } else { 0x10 });
    // mod=10 (disp32), reg, rm=100 selects a SIB byte, which rsp requires.
    out.push(0x84 | ((reg & 7) << 3));
    out.push(0x24); // SIB: no index, base = rsp
    out.extend_from_slice(&disp.to_le_bytes());
}

/// `lea rsp, [rsp + delta]`, which adjusts the stack **without touching
/// the flags**. `sub rsp, n` would do the same arithmetic and destroy the
/// program's flags, which the trampoline has not saved yet at the point
/// it first needs to move the stack.
fn emit_lea_rsp(out: &mut Code, delta: i32) {
    out.extend_from_slice(&[0x48, 0x8d]);
    if (-128..=127).contains(&delta) {
        out.extend_from_slice(&[0x64, 0x24, delta as i8 as u8]);
    } else {
        out.extend_from_slice(&[0xa4, 0x24]);
        out.extend_from_slice(&delta.to_le_bytes());
    }
}

fn emit_push(out: &mut Code, reg: u8) {
    if reg >= 8 {
        out.push(0x41);
    }
    out.push(0x50 | (reg & 7));
}

fn emit_pop(out: &mut Code, reg: u8) {
    if reg >= 8 {
        out.push(0x41);
    }
    out.push(0x58 | (reg & 7));
}

/// Register numbers in the order the trampoline pushes them, chosen so
/// that they land in memory in the order `frame::gpr_slot` declares.
/// The stack grows down, so the last one pushed is the lowest address and
/// therefore slot zero.
const PUSH_ORDER: [u8; 15] = [
    15, 14, 13, 12, 11, 10, 9, 8, // r15 down to r8
    5, // rbp
    7, // rdi
    6, // rsi
    2, // rdx
    1, // rcx
    3, // rbx
    0, // rax, last, so it is slot zero
];

/// Build the one trampoline every stub calls.
///
/// On entry the return address is on the stack and the program's red zone
/// has already been stepped over by the stub, so the stack below is free.
pub fn build_trampoline(callee: u64) -> Code {
    let mut c = Code::new();

    // Save the general purpose registers. `push` does not affect the
    // flags, which is why they can be saved after these rather than
    // before.
    for r in PUSH_ORDER {
        emit_push(&mut c, r);
    }
    c.push(0x9c); // pushfq
    emit_lea_rsp(&mut c, -512); // room for the vector registers
    for r in 0..16u8 {
        emit_vmovups(&mut c, r, i32::from(r) * 32, true);
    }

    // rsp now points at the frame. Arguments: the frame, and the return
    // address, from which the trampoline's caller can be identified.
    c.extend_from_slice(&[0x48, 0x89, 0xe7]); // mov rdi, rsp
    c.extend_from_slice(&[0x48, 0x8b, 0xb4, 0x24]); // mov rsi, [rsp + disp32]
    c.extend_from_slice(&(PatchFrame::SIZE as i32).to_le_bytes());

    // Keep the frame address somewhere the call cannot disturb, align the
    // stack as the ABI requires, call, and put it back. r15's own value
    // is already saved, so it is free to use.
    c.extend_from_slice(&[0x49, 0x89, 0xe7]); // mov r15, rsp
    c.extend_from_slice(&[0x48, 0x83, 0xe4, 0xf0]); // and rsp, -16
    c.extend_from_slice(&[0x48, 0xb8]); // movabs rax, callee
    c.extend_from_slice(&callee.to_le_bytes());
    c.extend_from_slice(&[0xff, 0xd0]); // call rax
    c.extend_from_slice(&[0x4c, 0x89, 0xfc]); // mov rsp, r15

    for r in 0..16u8 {
        emit_vmovups(&mut c, r, i32::from(r) * 32, false);
    }
    emit_lea_rsp(&mut c, 512);
    c.push(0x9d); // popfq
    for r in PUSH_ORDER.iter().rev() {
        emit_pop(&mut c, *r);
    }
    c.push(0xc3); // ret
    c
}

/// Build one site's stub: step over the red zone, call the trampoline,
/// step back, and continue at the instruction after the one replaced.
fn build_stub(stub_addr: u64, trampoline: u64, resume: u64) -> [u8; STUB_SIZE] {
    let mut c = Code::new();
    // The red zone is 128 bytes below rsp that a leaf function may be
    // using. The `call` below would write its return address into it.
    emit_lea_rsp(&mut c, -128);
    let call_site = stub_addr + c.len() as u64;
    c.push(0xe8); // call rel32
    c.extend_from_slice(&((trampoline as i64 - (call_site as i64 + 5)) as i32).to_le_bytes());
    debug_assert_eq!(c.len(), STUB_RET_OFFSET, "the return address offset is assumed elsewhere");
    emit_lea_rsp(&mut c, 128);
    let jmp_site = stub_addr + c.len() as u64;
    c.push(0xe9); // jmp rel32
    c.extend_from_slice(&((resume as i64 - (jmp_site as i64 + 5)) as i32).to_le_bytes());

    let mut out = [0x90u8; STUB_SIZE]; // pad with nop
    out[..c.len()].copy_from_slice(&c);
    out
}

// ---------------------------------------------------------------------
// The arena, and putting a stub in it
// ---------------------------------------------------------------------

/// Total arena: one trampoline followed by a stub for every site.
const ARENA_SIZE: usize = 4096 + MAX_SITES * STUB_SIZE;

/// Reserve executable memory within reach of the code being patched.
///
/// A near jump reaches plus or minus two gigabytes, so the arena has to
/// land near the text it will be jumped to from. The kernel is asked for
/// somewhere close, with several fallbacks, and the result is checked:
/// memory that is out of range is worse than none, because a truncated
/// displacement would jump somewhere arbitrary.
fn map_arena(near: u64) -> Option<u64> {
    const MB: u64 = 1024 * 1024;
    let hints = [
        near.wrapping_sub(64 * MB),
        near.wrapping_add(64 * MB),
        near.wrapping_sub(512 * MB),
        near.wrapping_add(512 * MB),
        0, // let the kernel choose, then check the range
    ];
    for hint in hints {
        // SAFETY: a plain anonymous mapping request.
        let p = unsafe {
            libc::mmap(
                (hint & !0xfff) as *mut libc::c_void,
                ARENA_SIZE,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            // Writable and executable together is refused on hardened
            // kernels. Nothing can be patched there, which is a
            // performance loss and not a correctness one.
            continue;
        }
        let addr = p as u64;
        if in_jump_range(near, addr) && in_jump_range(near, addr + ARENA_SIZE as u64) {
            return Some(addr);
        }
        // SAFETY: unmapping exactly what was just mapped.
        unsafe { libc::munmap(p, ARENA_SIZE) };
    }
    None
}

/// Can a five-byte jump at `from` reach `to`?
fn in_jump_range(from: u64, to: u64) -> bool {
    let delta = (to as i64).wrapping_sub(from as i64);
    i32::try_from(delta).is_ok()
}

/// Create the arena and write the trampoline, once.
fn ensure_arena(near: u64) -> Option<(u64, u64)> {
    let base = ARENA.load(Ordering::Acquire);
    if base != 0 {
        return Some((base, STUBS.load(Ordering::Relaxed)));
    }
    if ARENA_FAILED.load(Ordering::Relaxed) {
        return None;
    }
    let Some(base) = map_arena(near) else {
        ARENA_FAILED.store(true, Ordering::Relaxed);
        return None;
    };
    let code = build_trampoline(phi512_patched as *const () as u64);
    // SAFETY: writing into memory this function just mapped.
    unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), base as *mut u8, code.len()) };
    let stubs = base + 4096;
    STUBS.store(stubs, Ordering::Relaxed);
    ARENA.store(base, Ordering::Release);
    Some((base, stubs))
}

// ---------------------------------------------------------------------
// Rewriting the site
// ---------------------------------------------------------------------

/// Make the page or pages holding `[addr, addr+len)` writable.
fn unprotect(addr: u64, len: usize) -> bool {
    let page = 4096u64;
    let start = addr & !(page - 1);
    let end = (addr + len as u64).div_ceil(page) * page;
    // SAFETY: mprotect on the program's own text.
    unsafe {
        libc::mprotect(
            start as *mut libc::c_void,
            (end - start) as usize,
            libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
        ) == 0
    }
}

fn reprotect(addr: u64, len: usize) {
    let page = 4096u64;
    let start = addr & !(page - 1);
    let end = (addr + len as u64).div_ceil(page) * page;
    // SAFETY: as above, putting back what is normal for text.
    unsafe {
        libc::mprotect(
            start as *mut libc::c_void,
            (end - start) as usize,
            libc::PROT_READ | libc::PROT_EXEC,
        );
    }
}

/// Held while a site is being rewritten.
///
/// Two threads can fault on the same instruction at the same time: it is
/// in a loop, and every thread running that loop reaches it. Without this
/// they would both decide to rewrite it, and their stores would
/// interleave into the same five bytes, which is how a correct emulator
/// with a correct rewrite protocol still produces a corrupt program.
///
/// Rewriting is rare, once per site for the life of the process, so a
/// single lock over all of it costs nothing measurable and removes a
/// whole class of question.
static PATCH_LOCK: AtomicBool = AtomicBool::new(false);

/// Rewrite one site so it jumps to a stub instead of faulting.
///
/// Returns whether it was patched. A refusal is never an error: the site
/// simply keeps faulting, which is slower and still correct.
pub fn try_patch(rip: u64, len: usize, bytes: &[u8]) -> bool {
    // A near jump is five bytes. Anything shorter cannot hold one, which
    // in practice means the VEX-encoded mask instructions.
    if len < 5 {
        if TOO_SHORT.fetch_add(1, Ordering::Relaxed) < 3 {
            crate::report_short(rip, len, bytes);
        }
        return false;
    }

    // One rewriter at a time. A thread that loses the race does nothing:
    // it has already emulated this instruction, so the program is correct
    // either way, and the site will be rewritten by whoever holds the
    // lock or by this thread the next time round the loop.
    if PATCH_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return false;
    }
    let patched = patch_locked(rip, len, bytes);
    PATCH_LOCK.store(false, Ordering::Release);
    patched
}

fn patch_locked(rip: u64, len: usize, bytes: &[u8]) -> bool {
    // Re-check under the lock. Another thread may have rewritten this
    // very site between this one faulting and getting here, and doing it
    // twice would write a jump over a jump.
    if site_at(rip).is_some() {
        return false;
    }
    let Some((_, stubs)) = ensure_arena(rip) else {
        return false;
    };

    let index = SITE_COUNT.load(Ordering::Relaxed) as usize;
    if index >= MAX_SITES {
        return false;
    }
    let stub_addr = stubs + (index * STUB_SIZE) as u64;
    if !in_jump_range(rip, stub_addr) {
        return false;
    }

    // Fill in the site before anything can reach it. The instruction's
    // own bytes have to be kept: the original is about to be overwritten.
    let site = &SITES[index];
    site.rip.store(rip, Ordering::Relaxed);
    site.len.store(len as u32, Ordering::Relaxed);
    for (i, b) in bytes.iter().take(len.min(16)).enumerate() {
        site.bytes[i].store(*b, Ordering::Relaxed);
    }
    // Decode once, here, rather than on every execution of the stub.
    // SAFETY: this site is not published yet, and `index` came from a
    // fetch_add so no other thread has it.
    let decoded = crate::decode_at(&bytes[..len.min(16)], rip);
    site.regs.store(vector_regs(&decoded), Ordering::Relaxed);
    unsafe { cache_insn(index, decoded) };
    site.state.store(SITE_PATCHING, Ordering::Release);
    // Make the site findable **before** the breakpoint goes in. A thread
    // that reaches the instruction mid-rewrite traps, and the breakpoint
    // handler has to be able to recognise the address as one of ours; if
    // the site were only published afterwards it would look like somebody
    // else's breakpoint and the process would die.
    SITE_COUNT.store(index as u32 + 1, Ordering::Release);

    let stub = build_stub(stub_addr, ARENA.load(Ordering::Relaxed), rip + len as u64);
    // SAFETY: the arena is this library's own writable, executable memory.
    unsafe { std::ptr::copy_nonoverlapping(stub.as_ptr(), stub_addr as *mut u8, STUB_SIZE) };

    if !unprotect(rip, len) {
        // The site is already published, and a thread that faults here
        // will find it and wait for it to become ready. Left as
        // `Patching` it never would, and that thread would spin for the
        // rest of the process's life. The instruction is still the
        // original, so it simply keeps faulting and being emulated.
        site.state.store(SITE_FAILED, Ordering::Release);
        return false;
    }

    // The three-step rewrite the kernel uses for the same problem.
    //
    // Another thread may be about to execute this instruction, and a
    // five-byte store is not atomic. A one-byte store is. So the first
    // byte becomes a breakpoint, which is a legal instruction boundary
    // for anyone who arrives mid-rewrite; then the rest is written, which
    // nobody can reach because the first byte stops them; then the first
    // byte becomes the jump, atomically. A thread that trapped in the
    // window is sent back to re-execute the finished jump.
    let p = rip as *mut u8;
    // SAFETY: the page was just made writable and the range is the
    // instruction the processor itself reported.
    unsafe {
        std::ptr::write_volatile(p, 0xcc);
        std::sync::atomic::fence(Ordering::SeqCst);

        let rel = (stub_addr as i64 - (rip as i64 + 5)) as i32;
        let disp = rel.to_le_bytes();
        for (i, b) in disp.iter().enumerate() {
            std::ptr::write_volatile(p.add(1 + i), *b);
        }
        // Anything left of the original instruction becomes a no-op, so
        // the site disassembles cleanly for anyone looking at it later.
        for i in 5..len {
            std::ptr::write_volatile(p.add(i), 0x90);
        }
        std::sync::atomic::fence(Ordering::SeqCst);
        std::ptr::write_volatile(p, 0xe9);
        std::sync::atomic::fence(Ordering::SeqCst);
    }

    reprotect(rip, len);
    site.state.store(SITE_READY, Ordering::Release);
    PATCHED.fetch_add(1, Ordering::Relaxed);
    true
}

/// Find the site whose first byte is `addr`, if it is one of ours.
/// Used by the breakpoint handler to tell our rewrite in progress from
/// somebody else's debugger.
pub fn site_at(addr: u64) -> Option<usize> {
    let n = (SITE_COUNT.load(Ordering::Acquire) as usize).min(MAX_SITES);
    (0..n).find(|&i| {
        matches!(SITES[i].state.load(Ordering::Acquire), SITE_PATCHING | SITE_READY) && SITES[i].rip.load(Ordering::Relaxed) == addr
    })
}

/// Wait for a site's rewrite to finish. The window is a few stores wide,
/// so this spins rather than sleeping.
pub fn await_ready(index: usize) {
    while SITES[index].state.load(Ordering::Acquire) == SITE_PATCHING {
        std::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------
// What a patched site calls

/// Instructions already decoded, one per site.
///
/// Decoding is the single most expensive thing a patched site used to do:
/// the bytes never change, so doing it again on every execution was
/// paying a decoder to reach the same answer several million times. The
/// decode now happens once, while the site is being rewritten.
///
/// An entry is written before its site is published and never written
/// again, so a reader that has seen `SITE_READY` is reading settled
/// memory.
struct InsnCache(std::cell::UnsafeCell<[std::mem::MaybeUninit<iced_x86::Instruction>; MAX_SITES]>);

// SAFETY: entries are written once, before the site they belong to is
// published with a release store, and only read after that site has been
// observed with an acquire load.
unsafe impl Sync for InsnCache {}

static INSNS: InsnCache = InsnCache(std::cell::UnsafeCell::new([const { std::mem::MaybeUninit::uninit() }; MAX_SITES]));

/// Which of `zmm0` to `zmm15` an instruction names.
///
/// Only these need moving between the program's real registers and the
/// emulator's view. `zmm16` to `zmm31` have no `ymm` alias, so nothing
/// outside this library can touch them and they never need syncing at
/// all.
fn vector_regs(insn: &iced_x86::Instruction) -> u32 {
    use iced_x86::{OpKind, Register};
    let mut m = 0u32;
    let mut note = |r: Register| {
        let idx = if r.is_zmm() {
            r as u32 - Register::ZMM0 as u32
        } else if r.is_ymm() {
            r as u32 - Register::YMM0 as u32
        } else if r.is_xmm() {
            r as u32 - Register::XMM0 as u32
        } else {
            return;
        };
        if idx < 16 {
            m |= 1 << idx;
        }
    };
    for i in 0..insn.op_count() {
        if insn.op_kind(i) == OpKind::Register {
            note(insn.op_register(i));
        }
    }
    // A gather or scatter carries its index vector in the SIB byte
    // rather than in an operand, so it would be missed above.
    note(insn.memory_index());
    m
}
/// Record the decoded form of a site's instruction.
///
/// # Safety
/// `index` must be a site not yet published, and must not be written
/// concurrently.
unsafe fn cache_insn(index: usize, insn: iced_x86::Instruction) {
    let slot = &mut (*INSNS.0.get())[index];
    slot.write(insn);
}

/// Read back a decoded instruction.
///
/// # Safety
/// `index` must name a site that has reached `SITE_READY`, which is what
/// guarantees the entry was written.
unsafe fn cached_insn(index: usize) -> iced_x86::Instruction {
    (*INSNS.0.get())[index].assume_init()
}
// ---------------------------------------------------------------------

/// Perform the instruction a patched site stands for.
///
/// Called from the trampoline with the frame it built and the address it
/// will return to, which identifies the site: stubs are a fixed size in
/// one arena, so the index is a division.
///
/// This runs in the program's own context, not in a signal handler, so it
/// may allocate and take locks. It still does neither, because it is on
/// the hot path.
extern "C" fn phi512_patched(frame: *mut PatchFrame, ret_addr: u64) {
    let stubs = STUBS.load(Ordering::Relaxed);
    if stubs == 0 || ret_addr < stubs {
        return;
    }
    let index = ((ret_addr - stubs) / STUB_SIZE as u64) as usize;
    if index >= MAX_SITES {
        return;
    }
    let site = &SITES[index];
    if site.state.load(Ordering::Acquire) == SITE_EMPTY {
        return;
    }

    // SAFETY: the site has been published, so its cached decode exists.
    let insn = unsafe { cached_insn(index) };

    // SAFETY: the trampoline built this frame directly below its own
    // stack, so the program's stack pointer is a fixed distance above it.
    let frame = unsafe { &mut *frame };
    let live: [[u8; 32]; 16] = frame.ymm;
    // SAFETY: as above.
    let mut cpu = unsafe { crate::frame::PatchCpu::new(frame) };

    let mask = site.regs.load(Ordering::Relaxed);
    let result = crate::state::tls::with(|st| {
        st.sync_in_masked(&live, mask);
        let r = crate::emulate::step(&insn, st, &mut cpu);
        if r.is_ok() {
            st.sync_out_masked(&mut cpu.frame.ymm, mask);
        }
        r
    });
    if let Err(e) = result {
        // The instruction was performed successfully once, from the fault
        // handler, before this site was rewritten, so the emulator knows
        // it. Refusing it now would be a data-dependent gap. Skipping it
        // silently is the one thing this library must never do: the
        // program would continue with a register it believes was written,
        // and the illusion of a complete register file would be broken
        // without a word. Stop instead, and say why.
        let msg = format!("phi512: a rewritten site failed: {}\n", e.0);
        // SAFETY: writing a byte buffer we own to stderr, then aborting.
        unsafe {
            libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
            libc::abort();
        }
    }
}

/// How many sites have been registered. Diagnostic only.
pub fn site_count() -> u32 {
    SITE_COUNT.load(Ordering::Acquire)
}

/// The registered site address closest to `addr`, for diagnostics when a
/// fault arrives somewhere unexpected.
pub fn nearest_site(addr: u64) -> u64 {
    let n = (SITE_COUNT.load(Ordering::Acquire) as usize).min(MAX_SITES);
    let mut best = 0u64;
    let mut dist = u64::MAX;
    for s in SITES.iter().take(n) {
        let r = s.rip.load(Ordering::Relaxed);
        let d = r.abs_diff(addr);
        if d < dist {
            dist = d;
            best = r;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter};

    fn disassemble(code: &[u8], ip: u64) -> Vec<String> {
        let mut d = Decoder::with_ip(64, code, ip, DecoderOptions::NONE);
        let mut f = IntelFormatter::new();
        let mut out = Vec::new();
        while d.can_decode() {
            let insn = d.decode();
            let mut s = String::new();
            f.format(&insn, &mut s);
            out.push(s);
        }
        out
    }

    /// The trampoline is hand-assembled, so it is disassembled here and
    /// checked instruction by instruction. A wrong byte in it would not
    /// produce a wrong answer, it would produce a crash inside somebody
    /// else's program, with a stack that makes no sense.
    #[test]
    fn the_trampoline_disassembles_to_what_it_should() {
        let code = build_trampoline(0x1234_5678_9abc_def0);
        let text = disassemble(&code, 0x4000);

        // Fifteen registers saved, in the order that lands them in slot
        // order in memory.
        assert_eq!(text[0], "push r15");
        assert_eq!(text[7], "push r8");
        assert_eq!(text[8], "push rbp");
        assert_eq!(text[14], "push rax", "rax is pushed last so it is slot zero");
        assert_eq!(text[15], "pushfq");
        assert_eq!(text[16], "lea rsp,[rsp-200h]", "512 bytes for the vector registers");
        assert_eq!(text[17], "vmovups [rsp],ymm0");
        assert_eq!(text[32], "vmovups [rsp+1E0h],ymm15");
        assert_eq!(text[33], "mov rdi,rsp", "the frame is the first argument");
        assert_eq!(text[34], "mov rsi,[rsp+280h]", "the return address sits just above the frame");
        assert_eq!(text[35], "mov r15,rsp");
        assert_eq!(text[36], "and rsp,0FFFFFFFFFFFFFFF0h", "the ABI wants a 16-byte aligned stack");
        assert_eq!(text[37], "mov rax,123456789ABCDEF0h");
        assert_eq!(text[38], "call rax");
        assert_eq!(text[39], "mov rsp,r15");
        assert_eq!(text[40], "vmovups ymm0,[rsp]");
        assert_eq!(text[55], "vmovups ymm15,[rsp+1E0h]");
        assert_eq!(text[56], "lea rsp,[rsp+200h]");
        assert_eq!(text[57], "popfq");
        assert_eq!(text[58], "pop rax", "restored in the mirror order");
        assert_eq!(text[72], "pop r15");
        assert_eq!(text[73], "ret");
        assert_eq!(text.len(), 74, "and nothing else");
    }

    /// The trampoline must leave the flags exactly as it found them,
    /// because an AVX-512 instruction does not change them and the
    /// program may branch on them straight afterwards. Every stack
    /// adjustment before `pushfq` therefore has to be `lea`, never `sub`.
    #[test]
    fn nothing_before_pushfq_touches_the_flags() {
        let code = build_trampoline(0);
        let text = disassemble(&code, 0x4000);
        let upto = text.iter().position(|t| t == "pushfq").expect("pushfq is there");
        for t in &text[..upto] {
            assert!(t.starts_with("push "), "only pushes may precede pushfq, found: {t}");
        }
    }

    #[test]
    fn a_stub_calls_the_trampoline_and_resumes_after_the_original() {
        let stub = build_stub(0x10_0000, 0x10_8000, 0x40_0006);
        let text = disassemble(&stub, 0x10_0000);
        assert_eq!(text[0], "lea rsp,[rsp-80h]", "step over the red zone before the call");
        assert_eq!(text[1], "call 0000000000108000h");
        assert_eq!(text[2], "lea rsp,[rsp+80h]");
        assert_eq!(text[3], "jmp 0000000000400006h", "resume after the instruction that was replaced");
    }

    /// The trampoline identifies its caller by the return address, so the
    /// offset of the instruction after the `call` has to be exactly what
    /// the index calculation assumes.
    #[test]
    fn the_return_address_lands_where_the_index_calculation_expects() {
        let stub = build_stub(0x10_0000, 0x10_8000, 0x40_0006);
        // lea is 5 bytes, call is 5: the return address is stub + 10.
        assert_eq!(STUB_RET_OFFSET, 10);
        let mut d = Decoder::with_ip(64, &stub, 0x10_0000, DecoderOptions::NONE);
        let _lea = d.decode();
        let call = d.decode();
        assert_eq!(call.next_ip(), 0x10_0000 + STUB_RET_OFFSET as u64);
    }
}
