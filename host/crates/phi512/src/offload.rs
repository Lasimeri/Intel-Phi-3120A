//! The seamless path: from a SIGILL on an AVX-512 instruction, a region
//! of the program executes on the card's vector units.
//!
//! `run` is called from the SIGILL handler. It finds the region around
//! the faulting instruction (`analyze`): every instruction reachable
//! from it by falling through and by direct branches, stopping at
//! anything the card cannot run (a call, a return, an indirect jump, a
//! VEX or SSE instruction the host executes natively, an AVX-512
//! instruction the rewriter refuses) which become the region's exits.
//! AVX-512 instructions are rewritten in place to MVEX (`avx512_xlate::
//! rewrite`), the few that need a sequence become jumps into a thunk
//! area, and `ud2` is written at every exit.
//!
//! The region runs in phases (`plan`): when it holds a loop the planner
//! can split, the prologue up to the loop's head, the loop across the
//! card's threads, and the epilogue to the region's exits; else the whole
//! region at once. For each phase the planner tries to resolve every
//! address the phase touches from the register file it starts with; when
//! it can, the card fetches exactly those pages and writes exactly the
//! stored ranges back (ranges mode); when it cannot, the card pages the
//! program's memory in as the code touches it and returns the lines that
//! changed (demand mode). Either way the host serves the mailbox while
//! it waits, then updates the frame with the register file at the exit.
//! Nothing is interpreted anywhere.
//!
//! Regions are cached by their entry address. One region runs at a time
//! (a mutex); other threads of the program keep running on the host
//! meanwhile, and a write they make to a line the region also writes is
//! lost. See offload.md.

use std::collections::BTreeMap;
use std::sync::atomic::{fence, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use avx512_xlate::rewrite::{rewrite, thunk_bytes, Rewrite};
use iced_x86::{CpuidFeature, Decoder, DecoderOptions, EncodingKind, FlowControl, Instruction, Register};
use phi_vpu::proto::*;
use phi_vpu::window::{wait_ready, Window};

use crate::plan::{self, Bound, Insns, Loop, Tracker, Val};
use crate::state::VState;

/// ucontext gregs indices (glibc x86-64).
const REG_R8: usize = 0;
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
/// x86 encoding order (rax rcx rdx rbx rsp rbp rsi rdi r8..r15) as gregs indices.
const ORDER: [usize; 16] = [
    REG_RAX,
    REG_RCX,
    REG_RDX,
    REG_RBX,
    REG_RSP,
    REG_RBP,
    REG_RSI,
    REG_RDI,
    REG_R8,
    REG_R8 + 1,
    REG_R8 + 2,
    REG_R8 + 3,
    REG_R8 + 4,
    REG_R8 + 5,
    REG_R8 + 6,
    REG_R8 + 7,
];

const MAX_INSNS: usize = 4096;
const MAX_REGIONS: usize = 64;
const PAGE: u64 = 4096;
/// Threads a split loop is spread across: the card's 57 cores.
const SPLIT_THREADS: u64 = 57;

/// A region ready to ship: the code chunk image (the program's 2 MiB
/// with the rewrites and `ud2` at the region's exits), the thunk area,
/// the instructions for planning, and the loop to split, if any.
struct Region {
    id: u64,
    entry: u64,
    lo: u64,
    hi: u64,
    code_addr: u64,
    code: Vec<u8>,
    thunk_addr: u64,
    thunk: Vec<u8>,
    insns: Insns,
    exits: Vec<u64>,
    lp: Option<Loop>,
    avx512: usize,
}

/// One mapping of this process, from /proc/self/maps.
#[derive(Clone, Copy)]
struct Map {
    lo: u64,
    hi: u64,
    r: bool,
    w: bool,
    x: bool,
    /// vvar, vdso, vsyscall: the kernel's, not to be copied either way.
    special: bool,
}

struct Card {
    w: Window,
    index: usize,
    regions: Vec<Region>,
    maps: Vec<Map>,
    next_id: u64,
    /// This process, to the card: its mapped chunks belong to one program.
    session: u64,
}

static CARD: Mutex<Option<Card>> = Mutex::new(None);

/// One phase's cost, for the verbose report.
pub struct PhaseStats {
    pub what: &'static str,
    pub entry: u64,
    pub exit: u64,
    pub mode: &'static str,
    pub threads: u32,
    pub ranges: usize,
    pub chunks: u32,
    pub pages_back: u32,
    pub faults: u32,
    pub fetch_us: u64,
    pub run_us: u64,
    pub wb_us: u64,
    /// The card's own view: doorbell seen to reply written.
    pub card_us: u64,
    pub total_us: u64,
}

/// What one dispatch did.
pub struct Stats {
    pub lo: u64,
    pub hi: u64,
    pub insns: usize,
    pub avx512: usize,
    pub cached: bool,
    pub phases: Vec<PhaseStats>,
    pub total_us: u64,
}

/// Open the card named by `PHI512_CARD` (else card 0) and check that a
/// worker is polling its window. Called once at load; a program whose
/// host has no card gets the error at its first AVX-512 instruction.
pub fn init() -> Result<usize, String> {
    let index: usize = match std::env::var("PHI512_CARD") {
        Ok(s) => s.parse().map_err(|_| format!("PHI512_CARD={s} is not a card index"))?,
        Err(_) => 0,
    };
    let path = phi_vfio::cards::hostmem_path(index);
    let len = std::fs::metadata(&path)
        .map_err(|e| {
            format!(
                "no host-memory window for card {index} at {}: {e} (is the card up?)",
                path.display()
            )
        })?
        .len() as usize;
    let w = Window::open(path.to_str().unwrap_or(""), len).map_err(|e| format!("{e:#}"))?;
    wait_ready(&w, std::time::Duration::from_secs(2)).map_err(|e| format!("card {index}: {e:#} (phi -c {index} vpu start)"))?;
    *CARD.lock().unwrap_or_else(|e| e.into_inner()) = Some(Card {
        w,
        index,
        regions: Vec::new(),
        maps: Vec::new(),
        next_id: 1,
        session: (std::process::id() as u64) << 32
            | std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0)
                & 0xffff_ffff,
    });
    Ok(index)
}

fn read_maps() -> Vec<Map> {
    let text = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    let mut v = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(range), Some(perms)) = (it.next(), it.next()) else {
            continue;
        };
        let Some((a, b)) = range.split_once('-') else { continue };
        let (Ok(lo), Ok(hi)) = (u64::from_str_radix(a, 16), u64::from_str_radix(b, 16)) else {
            continue;
        };
        let p = perms.as_bytes();
        // The kernel's own pages near the stack: not memory of the program's.
        let name = line.split_whitespace().nth(5).unwrap_or("");
        let special = name.starts_with("[vvar") || name.starts_with("[vsyscall") || name.starts_with("[vdso");
        v.push(Map {
            lo,
            hi,
            r: p[0] == b'r',
            w: p[1] == b'w',
            x: p[2] == b'x',
            special,
        });
    }
    v
}

/// Copy [addr, addr+len) of this process into `out`, the parts inside
/// readable mappings; zero elsewhere. True if anything was mapped.
fn copy_out(maps: &[Map], addr: u64, out: &mut [u8]) -> bool {
    out.fill(0);
    let end = addr + out.len() as u64;
    let mut any = false;
    for m in maps.iter().filter(|m| m.r && !m.special && m.lo < end && m.hi > addr) {
        let lo = m.lo.max(addr);
        let hi = m.hi.min(end);
        // Through the kernel rather than a plain load: a page the table
        // calls readable can still refuse (vvar, a guard page), and a fault
        // here would take the program down with no message.
        let local = libc::iovec {
            iov_base: out[(lo - addr) as usize..].as_mut_ptr() as *mut libc::c_void,
            iov_len: (hi - lo) as usize,
        };
        let remote = libc::iovec {
            iov_base: lo as *mut libc::c_void,
            iov_len: (hi - lo) as usize,
        };
        // SAFETY: both iovecs describe memory of this process; the kernel
        // checks the remote one.
        let n = unsafe { libc::process_vm_readv(libc::getpid(), &local, 1, &remote, 1, 0) };
        if n > 0 {
            any = true;
        }
    }
    any
}

/// A small read of the program's memory for the planner (a stack slot a
/// loop reloads a pointer from): the value, or nothing if unreadable.
fn read_program(addr: u64, size: usize) -> Option<u64> {
    let mut buf = [0u8; 8];
    let local = libc::iovec {
        iov_base: buf.as_mut_ptr() as *mut libc::c_void,
        iov_len: size.min(8),
    };
    let remote = libc::iovec {
        iov_base: addr as *mut libc::c_void,
        iov_len: size.min(8),
    };
    // SAFETY: as in copy_out.
    let n = unsafe { libc::process_vm_readv(libc::getpid(), &local, 1, &remote, 1, 0) };
    if n == size.min(8) as isize {
        Some(u64::from_le_bytes(buf))
    } else {
        None
    }
}

/// Apply a write-back: `n` table entries at the head of `slot`, their
/// pages after the table; only the lines each entry marks, through the
/// kernel so a page the program cannot write is skipped, not faulted on.
fn apply_pages(maps: &[Map], slot: &[u8], n: usize) -> i32 {
    let mut local: Vec<libc::iovec> = Vec::with_capacity(1024);
    let mut remote: Vec<libc::iovec> = Vec::with_capacity(1024);
    let flush = |local: &mut Vec<libc::iovec>, remote: &mut Vec<libc::iovec>| {
        if local.is_empty() {
            return;
        }
        // SAFETY: every iovec describes memory of this process; the kernel
        // checks the remote ones and stops at the first it cannot write.
        unsafe {
            libc::process_vm_writev(
                libc::getpid(),
                local.as_ptr(),
                local.len() as u64,
                remote.as_ptr(),
                remote.len() as u64,
                0,
            );
        }
        local.clear();
        remote.clear();
    };
    let mut any = false;
    for i in 0..n {
        let e = &slot[i * 16..i * 16 + 16];
        let addr = u64::from_le_bytes(e[..8].try_into().unwrap());
        let lines = u64::from_le_bytes(e[8..].try_into().unwrap());
        if !maps.iter().any(|m| m.w && !m.special && addr >= m.lo && addr + 4096 <= m.hi) {
            continue;
        }
        any = true;
        let page = &slot[WB_TABLE as usize + i * 4096..WB_TABLE as usize + (i + 1) * 4096];
        let mut line = 0;
        while line < 64 {
            if lines >> line & 1 == 0 {
                line += 1;
                continue;
            }
            let start = line;
            while line < 64 && lines >> line & 1 == 1 {
                line += 1;
            }
            let bytes = (line - start) * 64;
            local.push(libc::iovec {
                iov_base: page[start * 64..].as_ptr() as *mut libc::c_void,
                iov_len: bytes,
            });
            remote.push(libc::iovec {
                iov_base: (addr + start as u64 * 64) as *mut libc::c_void,
                iov_len: bytes,
            });
            if local.len() == 1024 {
                flush(&mut local, &mut remote);
            }
        }
    }
    flush(&mut local, &mut remote);
    if any {
        0
    } else {
        -1
    }
}

/// Can the card run this non-AVX-512 instruction as it is? Knights
/// Corner is an x86-64 core without SSE, AVX, CMOV, the BMI families,
/// LZCNT, and without the newer extensions (ISA reference 327364-001,
/// appendix B.2); thread-local storage lives elsewhere on the card, so a
/// segment prefix ends the region too.
fn card_can_run(insn: &Instruction) -> bool {
    if insn.segment_prefix() != Register::None {
        return false;
    }
    insn.cpuid_features().iter().all(|f| {
        matches!(
            f,
            CpuidFeature::INTEL8086
                | CpuidFeature::INTEL186
                | CpuidFeature::INTEL286
                | CpuidFeature::INTEL386
                | CpuidFeature::INTEL486
                | CpuidFeature::X64
                | CpuidFeature::MULTIBYTENOP
                | CpuidFeature::FPU
                | CpuidFeature::FPU287
                | CpuidFeature::FPU387
                | CpuidFeature::CPUID
                | CpuidFeature::TSC
                | CpuidFeature::CX8
                | CpuidFeature::POPCNT
        )
    })
}

/// Build the region around `entry`. `maps` locates the text.
fn analyze(id: u64, entry: u64, maps: &[Map]) -> Result<Region, String> {
    let text = maps
        .iter()
        .find(|m| m.x && m.r && entry >= m.lo && entry < m.hi)
        .copied()
        .ok_or_else(|| format!("{entry:#x} is not in an executable mapping"))?;
    let mut included: BTreeMap<u64, (Instruction, Option<Rewrite>)> = BTreeMap::new();
    let mut exits: Vec<u64> = Vec::new();
    let mut work: Vec<u64> = vec![entry];
    let mut avx512 = 0usize;
    while let Some(addr) = work.pop() {
        if included.contains_key(&addr) || exits.contains(&addr) {
            continue;
        }
        if addr < text.lo || addr >= text.hi || included.len() >= MAX_INSNS {
            exits.push(addr);
            continue;
        }
        let avail = ((text.hi - addr).min(15)) as usize;
        // SAFETY: [addr, addr+avail) is inside a readable, executable mapping.
        let bytes = unsafe { std::slice::from_raw_parts(addr as *const u8, avail) };
        let insn = Decoder::with_ip(64, bytes, addr, DecoderOptions::NONE).decode();
        if insn.is_invalid() {
            exits.push(addr);
            continue;
        }
        let rw = match insn.encoding() {
            EncodingKind::EVEX => match rewrite(&insn, bytes) {
                Ok(r) => {
                    avx512 += 1;
                    Some(r)
                }
                Err(e) => {
                    if addr == entry {
                        return Err(format!("the card cannot run {}: {}", e.text, e.reason));
                    }
                    exits.push(addr);
                    continue;
                }
            },
            EncodingKind::Legacy | EncodingKind::D3NOW => {
                if !card_can_run(&insn) {
                    exits.push(addr);
                    continue;
                }
                None
            }
            _ => {
                exits.push(addr);
                continue;
            }
        };
        match insn.flow_control() {
            FlowControl::Next => work.push(insn.next_ip()),
            FlowControl::UnconditionalBranch => work.push(insn.near_branch_target()),
            FlowControl::ConditionalBranch => {
                work.push(insn.near_branch_target());
                work.push(insn.next_ip());
            }
            _ => {
                // call, ret, indirect jump, syscall, int, hlt, ud2: the host's
                exits.push(addr);
                continue;
            }
        }
        included.insert(addr, (insn, rw));
    }
    if included.is_empty() {
        return Err("empty region".into());
    }
    let lo = *included.keys().next().unwrap();
    let hi = included.iter().map(|(a, (i, _))| a + i.len() as u64).max().unwrap();
    for &e in &exits {
        if let Some((a, (i, _))) = included.range(..=e).next_back() {
            if e > *a && e < a + i.len() as u64 {
                return Err(format!("an exit at {e:#x} falls inside the instruction at {a:#x}"));
            }
        }
    }
    let code_addr = lo & !(EXEC_CHUNK - 1);
    if hi > code_addr + EXEC_CHUNK {
        return Err(format!(
            "the region {lo:#x}..{hi:#x} spans more than one {} KiB chunk",
            EXEC_CHUNK >> 10
        ));
    }
    // The whole chunk of the program around the region: only its pages
    // with code are sent, but data sharing them stays real.
    let mut code = vec![0u8; EXEC_CHUNK as usize];
    copy_out(maps, code_addr, &mut code);
    // The thunk area: a free, page-aligned stretch of this process's
    // address space beyond the code chunk, within reach of a rel32. Its
    // first kilobyte is the card's: entry stubs, one per thread.
    let thunk_addr = free_range(maps, (code_addr + EXEC_CHUNK).max(text.hi), 65536, lo)?;
    // The card owns the first kilobyte (an entry stub per thread) and the
    // slot after it (the split loop exit jump); host thunks follow.
    let mut thunk: Vec<u8> = vec![0xcc; 16 * EXEC_MAX_THREADS + 16];
    for (a, (i, rw)) in &included {
        match rw {
            None => {}
            Some(Rewrite::InPlace(v)) => {
                let o = (a - code_addr) as usize;
                code[o..o + v.len()].copy_from_slice(v);
            }
            Some(Rewrite::Thunk(seq)) => {
                let at = thunk_addr + thunk.len() as u64;
                let (t, s) = thunk_bytes(seq, at, *a, i.len(), i.next_ip());
                let o = (a - code_addr) as usize;
                code[o..o + s.len()].copy_from_slice(&s);
                thunk.extend_from_slice(&t);
                while thunk.len() % 16 != 0 {
                    thunk.push(0xcc);
                }
            }
        }
    }
    if thunk.len() as u64 > EXEC_THUNK_MAX {
        return Err("more thunk code than the thunk area holds".into());
    }
    for &e in &exits {
        if e >= code_addr && e + 2 <= code_addr + EXEC_CHUNK {
            let o = (e - code_addr) as usize;
            code[o] = 0x0f;
            code[o + 1] = 0x0b; // ud2
        }
    }
    thunk.resize(((thunk.len() as u64 + PAGE - 1) & !(PAGE - 1)) as usize, 0xcc);
    let insns: Insns = included.iter().map(|(a, (i, _))| (*a, *i)).collect();
    let lp = plan::find_loop(&insns, entry, &exits);
    Ok(Region {
        id,
        entry,
        lo,
        hi,
        code_addr,
        code,
        thunk_addr,
        thunk,
        insns,
        exits,
        lp,
        avx512,
    })
}

/// A free page-aligned range of `len` bytes at or above `from`, within
/// 2 GiB of `near`.
fn free_range(maps: &[Map], from: u64, len: u64, near: u64) -> Result<u64, String> {
    let mut at = (from + PAGE - 1) & !(PAGE - 1);
    let mut sorted: Vec<&Map> = maps.iter().collect();
    sorted.sort_by_key(|m| m.lo);
    loop {
        if at + len > near + (1 << 31) - (1 << 20) {
            return Err("no free range for the thunk area within reach of the region".into());
        }
        match sorted.iter().find(|m| m.lo < at + len && m.hi > at) {
            None => return Ok(at),
            Some(m) => at = (m.hi + PAGE - 1) & !(PAGE - 1),
        }
    }
}

/// Copy [addr, addr+len) of this process straight into the window at
/// `dst`, the parts inside readable mappings; zero elsewhere. True if
/// anything was mapped. One kernel copy, no staging.
fn copy_out_to(maps: &[Map], addr: u64, dst: *mut u8, len: usize) -> bool {
    // SAFETY: dst..dst+len is inside the window mapping (the caller checked).
    unsafe { std::ptr::write_bytes(dst, 0, len) };
    let end = addr + len as u64;
    let mut any = false;
    for m in maps.iter().filter(|m| m.r && !m.special && m.lo < end && m.hi > addr) {
        let lo = m.lo.max(addr);
        let hi = m.hi.min(end);
        let local = libc::iovec {
            // SAFETY: inside the window, as above.
            iov_base: unsafe { dst.add((lo - addr) as usize) } as *mut libc::c_void,
            iov_len: (hi - lo) as usize,
        };
        let remote = libc::iovec {
            iov_base: lo as *mut libc::c_void,
            iov_len: (hi - lo) as usize,
        };
        // SAFETY: both iovecs describe memory of this process; the kernel checks the remote one.
        let n = unsafe { libc::process_vm_readv(libc::getpid(), &local, 1, &remote, 1, 0) };
        if n > 0 {
            any = true;
        }
    }
    any
}

/// Serve the mailbox until the card answers request `seq`. `ranges` are
/// the phase's declared ranges: after a fetch of one piece of a range the
/// next piece is copied into the other slot before the card asks, so the
/// host's copy overlaps the card's DMA. A write-back is acknowledged on
/// receipt and applied after: the card's next DMA overlaps the apply, and
/// the following acknowledgement tells it the slot is free.
fn wait_serving(w: &Window, maps: &[Map], ranges: &[Range], seq: u64) -> Result<Reply, String> {
    let start = Instant::now();
    let mut ahead: Option<(u64, u64, u32)> = None;
    loop {
        let rep: Reply = w.read(OFF_REPLY);
        if rep.seq == seq {
            return Ok(rep);
        }
        let m: Mail = w.read(OFF_MAIL);
        if m.seq != m.ack {
            match m.kind {
                MAIL_FETCH => {
                    let len = (m.len as usize).min(EXEC_CHUNK as usize);
                    let slot_off = OFF_EXEC_FETCH + u64::from(m.slot & 1) * EXEC_CHUNK;
                    // Already there from the prefetch, or copied now.
                    let status = if ahead == Some((m.addr, m.len, m.slot & 1)) || copy_out_to(maps, m.addr, w.ptr(slot_off, len), len) {
                        0
                    } else {
                        -1
                    };
                    w.write(OFF_MAIL + 40, status);
                    fence(Ordering::SeqCst);
                    w.write(OFF_MAIL + 32, m.seq);
                    fence(Ordering::SeqCst);
                    // The next piece of the same range, into the other slot.
                    ahead = None;
                    if let Some(r) = ranges.iter().find(|r| m.addr >= r.addr && m.addr < r.addr + r.len) {
                        let dense_write = r.flags & RANGE_DENSE != 0;
                        let next = m.addr + m.len;
                        let range_end = (r.addr + r.len + PAGE - 1) & !(PAGE - 1);
                        if !dense_write && m.len >= EXEC_CHUNK / 2 && next < range_end {
                            let stop = ((next & !(EXEC_CHUNK - 1)) + EXEC_CHUNK).min(range_end);
                            let nlen = (stop - next) as usize;
                            let other = (m.slot & 1) ^ 1;
                            copy_out_to(maps, next, w.ptr(OFF_EXEC_FETCH + u64::from(other) * EXEC_CHUNK, nlen), nlen);
                            ahead = Some((next, nlen as u64, other));
                        }
                    }
                }
                MAIL_WRITEBACK => {
                    w.write(OFF_MAIL + 40, 0i32);
                    fence(Ordering::SeqCst);
                    w.write(OFF_MAIL + 32, m.seq);
                    fence(Ordering::SeqCst);
                    let n = (m.len as usize).min(WB_MAX_PAGES as usize);
                    let total = WB_TABLE as usize + n * 4096;
                    let off = OFF_EXEC_WB + u64::from(m.slot & 1) * EXEC_CHUNK;
                    // SAFETY: the slot lies inside the window mapping.
                    let slot = unsafe { std::slice::from_raw_parts(w.ptr(off, total), total) };
                    apply_pages(maps, slot, n);
                }
                _ => {
                    w.write(OFF_MAIL + 40, -1i32);
                    fence(Ordering::SeqCst);
                    w.write(OFF_MAIL + 32, m.seq);
                    fence(Ordering::SeqCst);
                }
            }
            continue;
        }
        if start.elapsed().as_secs() > 60 {
            return Err(format!("the card did not finish the phase within 60 s (request {seq})"));
        }
        std::hint::spin_loop();
    }
}

/// How a phase runs: the card pages memory in as needed, or every
/// address is known and the loop may be split.
enum Mode {
    Demand,
    Ranges {
        /// (addr, len, written, read, dense)
        ranges: Vec<(u64, u64, bool, bool, bool)>,
        split: Option<Split>,
    },
}

struct Split {
    threads: u32,
    ind: usize,
    bound_reg: usize,
    step: i64,
    start: u64,
    end: u64,
    exit: u64,
}

/// Plan a phase from `from` (the register file `regs`) over the
/// instructions in `[from, to)`: ranges mode when every address resolves.
fn plan_phase(region: &Region, regs: &Regs, from: u64, to: u64, outer: Option<(usize, u64, u64, u64)>) -> (Mode, Option<Tracker>) {
    let mut t = Tracker::new(regs.gpr, read_program);
    t.run(&region.insns, from, to, outer);
    if !t.resolved {
        return (Mode::Demand, None);
    }
    let ranges = t.merged();
    if ranges.len() > EXEC_MAX_RANGES {
        return (Mode::Demand, None);
    }
    (Mode::Ranges { ranges, split: None }, Some(t))
}

/// The pages of the code chunk a phase needs: the region's code, its
/// exits, and the phase's own exit, each with the region's image and
/// `ud2` at `extra_exit`.
fn code_pages(region: &Region, extra_exit: Option<u64>) -> Result<Vec<(u64, Vec<u8>)>, String> {
    let mut pages: Vec<u64> = Vec::new();
    let mut want = |a: u64| {
        let p = a & !(PAGE - 1);
        if p >= region.code_addr && p < region.code_addr + EXEC_CHUNK && !pages.contains(&p) {
            pages.push(p);
        }
    };
    let mut a = region.lo & !(PAGE - 1);
    while a < region.hi {
        want(a);
        a += PAGE;
    }
    for &e in &region.exits {
        want(e);
        want(e + 1);
    }
    if let Some(e) = extra_exit {
        want(e);
        want(e + 1);
    }
    if pages.len() > EXEC_MAX_PAGES {
        return Err(format!(
            "the region needs {} code pages, more than the {} a phase carries",
            pages.len(),
            EXEC_MAX_PAGES
        ));
    }
    pages.sort();
    let mut out = Vec::new();
    for p in pages {
        let o = (p - region.code_addr) as usize;
        let mut bytes = region.code[o..o + PAGE as usize].to_vec();
        if let Some(e) = extra_exit {
            for (k, b) in [0x0fu8, 0x0b].iter().enumerate() {
                let x = e + k as u64;
                if x >= p && x < p + PAGE {
                    bytes[(x - p) as usize] = *b;
                }
            }
        }
        out.push((p, bytes));
    }
    Ok(out)
}

/// Run one phase on the card and return the descriptor the card filled.
#[allow(clippy::too_many_arguments)]
fn dispatch(
    w: &Window,
    maps: &[Map],
    region: &Region,
    regs: Regs,
    entry: u64,
    extra_exit: Option<u64>,
    mode: &Mode,
    flags: u32,
    index: usize,
    session: u64,
) -> Result<(Exec, bool, u64), String> {
    let pages = code_pages(region, extra_exit)?;
    let mut desc = Exec {
        region_id: region.id,
        mode: MODE_DEMAND,
        flags,
        code_addr: region.code_addr,
        thunk_addr: region.thunk_addr,
        thunk_len: region.thunk.len() as u64,
        region_lo: region.lo,
        region_hi: region.hi,
        entry,
        code_pages: pages.len() as u32,
        threads: 1,
        regs,
        reserved: [session, 0, 0, 0],
        ..Exec::default()
    };
    for (i, (p, _)) in pages.iter().enumerate() {
        desc.code_page[i] = *p;
    }
    if let Mode::Ranges { ranges, split } = mode {
        desc.mode = MODE_RANGES;
        desc.nranges = ranges.len() as u32;
        for (i, &(a, l, wr, rd, dense)) in ranges.iter().enumerate() {
            desc.ranges[i] = Range {
                addr: a,
                len: l,
                flags: if wr { RANGE_WRITE } else { 0 } | if wr && !rd && dense { RANGE_DENSE } else { 0 },
                pad: 0,
            };
        }
        if let Some(s) = split {
            desc.threads = s.threads;
            desc.ind_reg = s.ind as u32;
            desc.bound_reg = s.bound_reg as u32;
            desc.step = s.step;
            desc.ind_start = s.start;
            desc.ind_end = s.end;
            desc.loop_exit = s.exit;
        }
    }
    // One bundle, one block read on the card: the descriptor (written again
    // once the mode is settled), the thunk area, the code pages.
    let thunk_at = OFF_EXEC_CODE + EXEC_DESC_PAGES * PAGE;
    w.put(thunk_at, &region.thunk);
    for (i, (_, bytes)) in pages.iter().enumerate() {
        w.put(thunk_at + region.thunk.len() as u64 + i as u64 * PAGE, bytes);
    }
    // The planner is an optimisation, never a correctness requirement: if
    // the phase touches an address the planner did not predict, the card
    // says so without having written anything back, and the same phase
    // runs again with the card paging memory in as it goes.
    let mut fell_back = false;
    loop {
        match submit(w, maps, &desc, index, region, entry) {
            Ok((out, card_us)) => return Ok((out, fell_back, card_us)),
            Err(e) => {
                if desc.mode == MODE_RANGES && e.starts_with("RETRY") {
                    desc.mode = MODE_DEMAND;
                    desc.nranges = 0;
                    desc.threads = 1;
                    fell_back = true;
                    continue;
                }
                return Err(e.trim_start_matches("RETRY: ").to_string());
            }
        }
    }
}

/// One submission of a prepared descriptor. A fault in ranges mode comes
/// back marked RETRY, which `dispatch` turns into a demand-mode run.
fn submit(w: &Window, maps: &[Map], desc: &Exec, index: usize, region: &Region, entry: u64) -> Result<(Exec, u64), String> {
    let desc = *desc;
    w.write(OFF_EXEC, desc);
    w.write(OFF_EXEC_CODE as usize, desc);
    let seq = w.read::<u64>(OFF_REQ) + 1;
    let req = Request {
        seq: seq - 1,
        kernel: K_EXEC,
        threads: desc.threads,
        ..Request::default()
    };
    w.write(OFF_REQ, req);
    fence(Ordering::SeqCst);
    w.write(OFF_REQ, seq);
    fence(Ordering::SeqCst);
    let rep = wait_serving(w, maps, &desc.ranges[..desc.nranges as usize], seq)?;
    if rep.status != OK {
        return Err(format!("card {index}: {}", status_name(rep.status)));
    }
    let out: Exec = w.read(OFF_EXEC);
    match out.exit_kind {
        EXIT_LEFT => Ok((out, rep.total_ns / 1000)),
        EXIT_FAULT if desc.mode == MODE_RANGES => Err(format!(
            "RETRY: the phase at {entry:#x} touched {:#x} at {:#x}, outside what the planner worked out",
            out.fault_addr, out.exit_rip
        )),
        EXIT_FAULT => Err(format!(
            "card {index}: the phase at {entry:#x} touched {:#x} at {:#x}, which this process has not mapped",
            out.fault_addr, out.exit_rip
        )),
        EXIT_ILLEGAL => Err(format!(
            "card {index}: the card refused the instruction at {:#x} inside the region at {:#x} (a translation the card does not accept)",
            out.exit_rip, region.entry
        )),
        EXIT_COLLISION => Err(format!(
            "card {index}: the program's address {:#x} is already in use on the card (the worker's own mappings); retry, or move the worker",
            out.fault_addr
        )),
        EXIT_SPLIT => Err(format!(
            "RETRY: thread {} of the split loop at {entry:#x} left at {:#x} instead of {:#x}",
            out.exit_thread, out.exit_rip, out.loop_exit
        )),
        k => Err(format!("card {index}: exit kind {k} (a limit of the card's engine was reached)")),
    }
}

fn stats_of(what: &'static str, entry: u64, out: &Exec, mode: &Mode, fell_back: bool, card_us: u64, t0: Instant) -> PhaseStats {
    let (mode_name, ranges, threads) = match (mode, fell_back) {
        (_, true) => ("demand after the planner missed an address", 0, 1),
        (Mode::Demand, _) => ("demand", 0, 1),
        (Mode::Ranges { ranges, split }, _) => (if split.is_some() { "split" } else { "ranges" }, ranges.len(), out.threads_ran),
    };
    PhaseStats {
        what,
        entry,
        exit: out.exit_rip,
        mode: mode_name,
        threads,
        ranges,
        chunks: out.chunks,
        pages_back: out.dirty,
        faults: out.faults,
        fetch_us: out.fetch_ns / 1000,
        run_us: out.run_ns / 1000,
        wb_us: out.wb_ns / 1000,
        card_us,
        total_us: t0.elapsed().as_micros() as u64,
    }
}

/// Run the region at `rip` on the card; on success the frame's registers
/// and `st` hold the state at the exit and the frame's rip is the exit.
///
/// # Safety
///
/// `uc` must be the ucontext the kernel handed the SIGILL handler for the
/// fault at `rip`.
pub unsafe fn run(uc: *mut libc::ucontext_t, rip: u64, st: &mut VState) -> Result<Stats, String> {
    let t0 = Instant::now();
    let mut guard = CARD.lock().unwrap_or_else(|e| e.into_inner());
    let card = guard.as_mut().ok_or("no card")?;
    card.maps = read_maps();
    let mut cached = true;
    let idx = match card.regions.iter().position(|r| r.entry == rip) {
        Some(i) => i,
        None => {
            cached = false;
            let id = card.next_id;
            card.next_id += 1;
            let r = analyze(id, rip, &card.maps)?;
            if card.regions.len() >= MAX_REGIONS {
                card.regions.remove(0);
            }
            card.regions.push(r);
            card.regions.len() - 1
        }
    };
    let Card {
        w,
        index,
        regions,
        maps,
        session,
        ..
    } = &mut *card;
    let region = &regions[idx];
    let index = *index;
    let session = *session;
    // SAFETY: the kernel handed us this frame.
    let g = unsafe { &mut (*uc).uc_mcontext.gregs };
    let mut regs = Regs::default();
    for (i, &gi) in ORDER.iter().enumerate() {
        regs.gpr[i] = g[gi] as u64;
    }
    regs.rflags = g[REG_EFL] as u64;
    regs.rip = rip;
    regs.zmm = st.zmm;
    for i in 0..8 {
        regs.k[i] = st.k[i] as u16;
    }
    let mut phases: Vec<PhaseStats> = Vec::new();
    let mut flags = EXEC_FIRST;
    let mut out: Exec;

    match region.lp {
        None => {
            // One phase. Every address resolves only when the phase starts at
            // the region's first instruction (code before the entry runs on
            // later values); else demand mode.
            let mode = if region.entry == region.lo {
                plan_phase(region, &regs, region.lo, region.hi, None).0
            } else {
                Mode::Demand
            };
            let p0 = Instant::now();
            let (o, fb, cu) = dispatch(w, maps, region, regs, rip, None, &mode, EXEC_FIRST | EXEC_FINAL, index, session)?;
            out = o;
            phases.push(stats_of("region", rip, &out, &mode, fb, cu, p0));
        }
        Some(lp) => {
            let has_epilogue = region.insns.range(lp.exit..).next().is_some();
            let mut state = regs;
            let mut at = rip;
            // The prologue: from the fault to the loop's head, once.
            if at != lp.head {
                let to = if at < lp.head { lp.head } else { lp.exit };
                let mode = plan_phase(region, &state, at, to, None).0;
                let p0 = Instant::now();
                let (o, fb, cu) = dispatch(w, maps, region, state, at, Some(lp.head), &mode, flags, index, session)?;
                out = o;
                phases.push(stats_of("prologue", at, &out, &mode, fb, cu, p0));
                flags = 0;
                state = out.regs;
                at = out.exit_rip;
            }
            if at == lp.head {
                // The loop: its trip count from the induction register now.
                let i0 = state.gpr[lp.ind];
                let bound = match lp.bound {
                    Bound::Imm(b) => b,
                    Bound::Reg(r) => state.gpr[r],
                };
                let n = plan::iterations(&lp, i0, bound);
                let mode = match n {
                    Some(n) => {
                        let last = i0.wrapping_add((lp.step as u64).wrapping_mul(n - 1));
                        let (vlo, vhi) = if lp.step > 0 { (i0, last) } else { (last, i0) };
                        let (mode, tracker) =
                            plan_phase(region, &state, lp.head, lp.exit, Some((lp.ind, vlo, vhi, lp.step.unsigned_abs())));
                        match (mode, tracker, lp.bound) {
                            (Mode::Ranges { ranges, .. }, Some(t), Bound::Reg(bound_reg)) => {
                                let split = match plan::splittable(&region.insns, &lp, &t) {
                                    Ok(()) if n > 1 => Some(Split {
                                        threads: n.min(SPLIT_THREADS) as u32,
                                        ind: lp.ind,
                                        bound_reg,
                                        step: lp.step,
                                        start: i0,
                                        end: i0.wrapping_add((lp.step as u64).wrapping_mul(n)),
                                        exit: lp.exit,
                                    }),
                                    _ => None,
                                };
                                Mode::Ranges { ranges, split }
                            }
                            (m, _, _) => m,
                        }
                    }
                    None => Mode::Demand,
                };
                let p0 = Instant::now();
                let final_here = !has_epilogue;
                let (o, fb, cu) = dispatch(
                    w,
                    maps,
                    region,
                    state,
                    lp.head,
                    Some(lp.exit),
                    &mode,
                    flags | if final_here { EXEC_FINAL } else { 0 },
                    index,
                    session,
                )?;
                out = o;
                phases.push(stats_of("loop", lp.head, &out, &mode, fb, cu, p0));
                flags = 0;
                state = out.regs;
                at = out.exit_rip;
                if at != lp.exit || final_here {
                    // Left the region from inside the loop, or nothing follows.
                    let _ = flags;
                    out.regs = state;
                    out.exit_rip = at;
                    return finish(g, st, &out, region, cached, phases, t0);
                }
            }
            // The epilogue: from wherever the previous phase left to the region's exits.
            let mode = plan_phase(region, &state, at, region.hi, None).0;
            let p0 = Instant::now();
            let (o, fb, cu) = dispatch(w, maps, region, state, at, None, &mode, flags | EXEC_FINAL, index, session)?;
            out = o;
            phases.push(stats_of("epilogue", at, &out, &mode, fb, cu, p0));
        }
    }
    finish(g, st, &out, region, cached, phases, t0)
}

/// The frame and the vector state get the exit register file.
fn finish(
    g: &mut [i64; 23],
    st: &mut VState,
    out: &Exec,
    region: &Region,
    cached: bool,
    phases: Vec<PhaseStats>,
    t0: Instant,
) -> Result<Stats, String> {
    for (i, &gi) in ORDER.iter().enumerate() {
        g[gi] = out.regs.gpr[i] as i64;
    }
    g[REG_EFL] = out.regs.rflags as i64;
    g[REG_RIP] = out.exit_rip as i64;
    st.zmm = out.regs.zmm;
    for i in 0..8 {
        st.k[i] = u64::from(out.regs.k[i]);
    }
    Ok(Stats {
        lo: region.lo,
        hi: region.hi,
        insns: region.insns.len(),
        avx512: region.avx512,
        cached,
        phases,
        total_us: t0.elapsed().as_micros() as u64,
    })
}

#[allow(dead_code)]
fn unused(_: Val) {}
