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
//! area, and `ud2` is written at every exit. The chunk of the program's
//! text holding the region, the thunk area, and the register file (the
//! frame's integer registers and flags, the library's zmm and mask
//! state) go to the card through the window; the card maps the code at
//! the program's own addresses, pages the rest of memory in through the
//! mailbox this function serves while it waits, and returns the
//! register file at the exit and every chunk it wrote (card/vpu/
//! vpu_exec.h). The frame is updated and the program resumes at the
//! exit. Nothing is interpreted anywhere.
//!
//! Regions are cached by their entry address. One region runs at a time
//! (a mutex); other threads of the program keep running on the host
//! meanwhile, and writes they make to chunks the card holds are lost
//! when the card writes those chunks back: a single-threaded
//! program's view, for now. See offload.md.

use std::collections::BTreeMap;
use std::sync::atomic::{fence, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use avx512_xlate::rewrite::{rewrite, thunk_bytes, Rewrite};
use iced_x86::{CpuidFeature, Decoder, DecoderOptions, EncodingKind, FlowControl, Instruction, Register};
use phi_vpu::proto::*;
use phi_vpu::window::{wait_ready, Window};

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

/// A region ready to ship: the code chunk as prepared, the thunk area
/// (its first 16 bytes are the entry stub, written per dispatch), bounds.
struct Region {
    entry: u64,
    lo: u64,
    hi: u64,
    code_addr: u64,
    code: Vec<u8>,
    thunk_addr: u64,
    thunk: Vec<u8>,
    insns: usize,
    avx512: usize,
    _exits: usize,
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
}

static CARD: Mutex<Option<Card>> = Mutex::new(None);

/// What one dispatch cost, for the verbose report.
pub struct Stats {
    pub lo: u64,
    pub hi: u64,
    pub exit: u64,
    pub insns: usize,
    pub avx512: usize,
    pub chunks: u32,
    pub dirty: u32,
    pub faults: u32,
    pub fetch_us: u64,
    pub run_us: u64,
    pub wb_us: u64,
    pub total_us: u64,
    pub cached: bool,
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

/// The reverse: `data` into [addr, addr+len), writable parts only.
#[allow(dead_code)]
fn copy_in(maps: &[Map], addr: u64, data: &[u8]) -> bool {
    let end = addr + data.len() as u64;
    let mut any = false;
    for m in maps.iter().filter(|m| m.w && !m.special && m.lo < end && m.hi > addr) {
        let lo = m.lo.max(addr);
        let hi = m.hi.min(end);
        let local = libc::iovec {
            iov_base: data[(lo - addr) as usize..].as_ptr() as *mut libc::c_void,
            iov_len: (hi - lo) as usize,
        };
        let remote = libc::iovec {
            iov_base: lo as *mut libc::c_void,
            iov_len: (hi - lo) as usize,
        };
        // SAFETY: as in copy_out; the kernel refuses a page that is not writable.
        let n = unsafe { libc::process_vm_writev(libc::getpid(), &local, 1, &remote, 1, 0) };
        if n > 0 {
            any = true;
        }
    }
    any
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
fn analyze(entry: u64, maps: &[Map]) -> Result<Region, String> {
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
            EncodingKind::VEX | EncodingKind::XOP | EncodingKind::MVEX => {
                exits.push(addr);
                continue;
            }
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
    // The whole chunk of the program around the region, since the card maps
    // it whole: data sharing it is real, only the region and its exits differ.
    let code_len = EXEC_CHUNK;
    let mut code = vec![0u8; code_len as usize];
    copy_out(maps, code_addr, &mut code);
    // The thunk area: a free, page-aligned stretch of this process's
    // address space beyond the code chunk, within reach of a rel32.
    let thunk_addr = free_range(maps, (code_addr + EXEC_CHUNK).max(text.hi), 65536, lo)?;
    let mut thunk: Vec<u8> = vec![0; 16]; // the entry stub, filled per dispatch
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
                // Keep sequences on 16-byte boundaries for the decoder's sake.
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
        if e >= code_addr && e + 2 <= code_addr + code_len {
            let o = (e - code_addr) as usize;
            code[o] = 0x0f;
            code[o + 1] = 0x0b; // ud2
        }
    }
    thunk.resize(((thunk.len() as u64 + PAGE - 1) & !(PAGE - 1)) as usize, 0xcc);
    Ok(Region {
        entry,
        lo,
        hi,
        code_addr,
        code,
        thunk_addr,
        thunk,
        insns: included.len(),
        avx512,
        _exits: exits.len(),
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

/// Serve the mailbox until the card answers request `seq`.
fn wait_serving(w: &Window, maps: &[Map], seq: u64) -> Result<Reply, String> {
    let mut chunk = vec![0u8; EXEC_CHUNK as usize];
    let start = Instant::now();
    loop {
        let rep: Reply = w.read(OFF_REPLY);
        if rep.seq == seq {
            return Ok(rep);
        }
        let m: Mail = w.read(OFF_MAIL);
        if m.seq != m.ack {
            let len = (m.len as usize).min(chunk.len());
            let status = match m.kind {
                MAIL_FETCH => {
                    let any = copy_out(maps, m.addr, &mut chunk[..len]);
                    w.put(OFF_EXEC_FETCH, &chunk[..len]);
                    if any {
                        0
                    } else {
                        -1
                    }
                }
                MAIL_WRITEBACK => {
                    // len is the page count: the table, then the pages.
                    let n = (m.len as usize).min(WB_MAX_PAGES as usize);
                    let total = WB_TABLE as usize + n * 4096;
                    w.get(OFF_EXEC_WB, &mut chunk[..total]);
                    apply_pages(maps, &chunk[..total], n)
                }
                _ => -1,
            };
            w.write(OFF_MAIL + 40, status);
            fence(Ordering::SeqCst);
            w.write(OFF_MAIL + 32, m.seq);
            fence(Ordering::SeqCst);
            continue;
        }
        if start.elapsed().as_secs() > 60 {
            return Err(format!("the card did not finish the region within 60 s (request {seq})"));
        }
        std::hint::spin_loop();
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
            let r = analyze(rip, &card.maps)?;
            if card.regions.len() >= MAX_REGIONS {
                card.regions.remove(0);
            }
            card.regions.push(r);
            card.regions.len() - 1
        }
    };
    let region = &mut card.regions[idx];
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
    // The entry stub: rax, then the faulting instruction.
    let stub = &mut region.thunk[..16];
    stub[0] = 0x48;
    stub[1] = 0xb8;
    stub[2..10].copy_from_slice(&regs.gpr[0].to_le_bytes());
    stub[10] = 0xe9;
    let rel = rip as i64 - (region.thunk_addr as i64 + 15);
    stub[11..15].copy_from_slice(&(rel as i32).to_le_bytes());
    stub[15] = 0xcc;
    let desc = Exec {
        code_addr: region.code_addr,
        code_len: region.code.len() as u64,
        thunk_addr: region.thunk_addr,
        thunk_len: region.thunk.len() as u64,
        region_lo: region.lo,
        region_hi: region.hi,
        entry: region.thunk_addr,
        reserved: [0; 9],
        exit_rip: 0,
        fault_addr: 0,
        exit_kind: 0,
        chunks: 0,
        dirty: 0,
        faults: 0,
        fetch_ns: 0,
        wb_ns: 0,
        run_ns: 0,
        reserved2: [0; 9],
        regs,
    };
    let w = &card.w;
    w.put(OFF_EXEC_CODE, &region.code);
    w.put(OFF_EXEC_THUNK, &region.thunk);
    w.write(OFF_EXEC, desc);
    let seq = w.read::<u64>(OFF_REQ) + 1;
    let req = Request {
        seq: seq - 1,
        kernel: K_EXEC,
        threads: 1,
        ..Request::default()
    };
    w.write(OFF_REQ, req);
    fence(Ordering::SeqCst);
    w.write(OFF_REQ, seq);
    fence(Ordering::SeqCst);
    let rep = wait_serving(w, &card.maps, seq)?;
    if rep.status != OK {
        return Err(format!("card {}: {}", card.index, status_name(rep.status)));
    }
    let out: Exec = w.read(OFF_EXEC);
    match out.exit_kind {
        EXIT_LEFT => {}
        EXIT_FAULT => {
            return Err(format!(
                "card {}: the region at {rip:#x} touched {:#x} at {:#x}, which this process has not mapped",
                card.index, out.fault_addr, out.exit_rip
            ))
        }
        EXIT_ILLEGAL => {
            return Err(format!(
                "card {}: the card refused the instruction at {:#x} inside the region at {rip:#x} (a translation the card does not accept)",
                card.index, out.exit_rip
            ))
        }
        EXIT_COLLISION => {
            return Err(format!(
                "card {}: the program's address {:#x} is already in use on the card (the worker's own mappings); retry, or move the worker",
                card.index, out.fault_addr
            ))
        }
        k => return Err(format!("card {}: exit kind {k} (limit reached)", card.index)),
    }
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
        exit: out.exit_rip,
        insns: region.insns,
        avx512: region.avx512,
        chunks: out.chunks,
        dirty: out.dirty,
        faults: out.faults,
        fetch_us: out.fetch_ns / 1000,
        run_us: out.run_ns / 1000,
        wb_us: out.wb_ns / 1000,
        total_us: t0.elapsed().as_micros() as u64,
        cached,
    })
}
