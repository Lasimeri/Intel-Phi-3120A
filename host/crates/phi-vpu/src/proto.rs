//! The host/card contract, mirrored from `card/vpu/vpu_proto.h`.
//!
//! The two files must agree byte for byte: the card reads what the host
//! writes at these offsets and nothing checks it at run time. Both carry
//! the same size assertions, the unit tests below pin every field offset,
//! and `tools/vpu-layout-check.c` compares the C side against the same
//! numbers.

/// Window offset of the card's readiness word.
pub const OFF_READY: usize = 0;
/// Window offset of the request descriptor.
pub const OFF_REQ: usize = 64;
/// Window offset of the reply descriptor.
pub const OFF_REPLY: usize = 256;
/// Where bulk data starts. Everything below is control words.
pub const OFF_DATA: u64 = 1 << 20;

/// What the card writes at `OFF_READY` while it is polling ("VPU_READ").
pub const MAGIC: u64 = 0x5650_555F_5245_4144;

/// Degree-30 Horner in float32: `out[i] = poly(in[i])`.
pub const K_POLY30: u32 = 1;

/// The card moves data with `O_DIRECT`, so every offset and every
/// transfer is a whole number of these.
pub const BLOCK: u64 = 4096;
/// The POLY30 kernel works in steps of this many elements.
pub const CHUNK: u64 = 128;

/// Round a byte count or offset up to a block boundary.
pub const fn round_up(x: u64) -> u64 {
    (x + BLOCK - 1) & !(BLOCK - 1)
}

/// `struct vpu_request`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// The host bumps this last; the card polls it.
    pub seq: u64,
    pub kernel: u32,
    /// How many card threads to spread the work across.
    pub threads: u32,
    /// Elements.
    pub n: u64,
    pub in_off: u64,
    pub out_off: u64,
    pub aux_off: u64,
    pub aux_len: u64,
}

/// `struct vpu_reply`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    /// Echoes the request's sequence number once the work is done.
    pub seq: u64,
    /// Time on the vector units alone.
    pub compute_ns: u64,
    /// From the doorbell being seen to the reply being written.
    pub total_ns: u64,
    /// Moving the input and coefficients onto the card.
    pub pull_ns: u64,
    /// Moving the output back.
    pub push_ns: u64,
    pub status: i32,
    /// Slices that actually ran.
    pub threads: i32,
}

pub const OK: i32 = 0;
pub const E_ALLOC: i32 = -1;
pub const E_PULL: i32 = -2;
pub const E_PUSH: i32 = -3;
pub const E_REQUEST: i32 = -4;
pub const E_KERNEL: i32 = -5;

/// A reply status as words.
pub fn status_name(status: i32) -> &'static str {
    match status {
        OK => "ok",
        E_ALLOC => "card could not reserve buffers",
        E_PULL => "card could not read the input from the window",
        E_PUSH => "card could not write the output (unaligned offset?)",
        E_REQUEST => "request rejected: n is zero or an offset is not block aligned",
        E_KERNEL => "unknown kernel number",
        _ => "unknown status",
    }
}

// The C header asserts the same two sizes.
const _: [(); 56] = [(); std::mem::size_of::<Request>()];
const _: [(); 48] = [(); std::mem::size_of::<Reply>()];

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    /// These are the numbers `tools/vpu-layout-check.c` prints for the C
    /// side. If either file changes, one of the two checks fails.
    #[test]
    fn request_layout_matches_the_c_header() {
        assert_eq!(offset_of!(Request, seq), 0);
        assert_eq!(offset_of!(Request, kernel), 8);
        assert_eq!(offset_of!(Request, threads), 12);
        assert_eq!(offset_of!(Request, n), 16);
        assert_eq!(offset_of!(Request, in_off), 24);
        assert_eq!(offset_of!(Request, out_off), 32);
        assert_eq!(offset_of!(Request, aux_off), 40);
        assert_eq!(offset_of!(Request, aux_len), 48);
    }

    #[test]
    fn reply_layout_matches_the_c_header() {
        assert_eq!(offset_of!(Reply, seq), 0);
        assert_eq!(offset_of!(Reply, compute_ns), 8);
        assert_eq!(offset_of!(Reply, total_ns), 16);
        assert_eq!(offset_of!(Reply, pull_ns), 24);
        assert_eq!(offset_of!(Reply, push_ns), 32);
        assert_eq!(offset_of!(Reply, status), 40);
        assert_eq!(offset_of!(Reply, threads), 44);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn control_words_do_not_share_a_cache_line() {
        assert!(OFF_REQ - OFF_READY >= 64);
        assert!(OFF_REPLY - OFF_REQ >= std::mem::size_of::<Request>());
        assert!(OFF_REPLY - OFF_REQ >= 64);
        assert!(OFF_DATA as usize >= OFF_REPLY + std::mem::size_of::<Reply>());
        assert_eq!(OFF_DATA % BLOCK, 0);
    }

    #[test]
    fn rounding_lands_on_blocks() {
        assert_eq!(round_up(0), 0);
        assert_eq!(round_up(1), 4096);
        assert_eq!(round_up(4096), 4096);
        assert_eq!(round_up(4097), 8192);
    }
}

/// Run a region of the host program on the card (card/vpu/vpu_exec.h).
pub const K_EXEC: u32 = 2;

// The seamless path (card/vpu/vpu_exec.h).
pub const EXEC_CHUNK: u64 = 2 << 20;
pub const EXEC_THUNK_MAX: u64 = 64 << 10;
pub const EXEC_MAX_RANGES: usize = 64;
pub const EXEC_MAX_PAGES: usize = 64;
pub const EXEC_MAX_THREADS: usize = 64;
/// The descriptor leads the bundle at `OFF_EXEC_CODE`: this many pages, then the thunk area, then the code pages.
pub const EXEC_DESC_PAGES: u64 = 2;
pub const OFF_MAIL: usize = 4096;
pub const OFF_EXEC: usize = 8192;
pub const OFF_EXEC_FETCH: u64 = 32 << 20;
pub const OFF_EXEC_CODE: u64 = OFF_EXEC_FETCH + 2 * EXEC_CHUNK;
pub const OFF_EXEC_WB: u64 = OFF_EXEC_CODE + EXEC_CHUNK;

/// The register file: zmm0..31, k0..7, then the integer registers in x86
/// encoding order (rax rcx rdx rbx rsp rbp rsi rdi r8..r15), flags, rip.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Regs {
    pub zmm: [[u8; 64]; 32],
    pub k: [u16; 8],
    pub pad: [u8; 48],
    pub gpr: [u64; 16],
    pub rflags: u64,
    pub rip: u64,
}

impl Default for Regs {
    fn default() -> Self {
        Regs {
            zmm: [[0; 64]; 32],
            k: [0; 8],
            pad: [0; 48],
            gpr: [0; 16],
            rflags: 0,
            rip: 0,
        }
    }
}

/// A range of the program's memory a phase touches.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Range {
    pub addr: u64,
    pub len: u64,
    pub flags: u32,
    pub pad: u32,
}
pub const RANGE_WRITE: u32 = 1;
/// Every byte written and none read: the card fetches only its edge pages.
pub const RANGE_DENSE: u32 = 2;

pub const MODE_DEMAND: u32 = 0;
pub const MODE_RANGES: u32 = 1;
pub const EXEC_FINAL: u32 = 1;
pub const EXEC_FIRST: u32 = 2;

pub const EXIT_LEFT: u32 = 0;
pub const EXIT_FAULT: u32 = 1;
pub const EXIT_ILLEGAL: u32 = 2;
pub const EXIT_COLLISION: u32 = 3;
pub const EXIT_LIMIT: u32 = 4;
pub const EXIT_SPLIT: u32 = 5;

/// The exec descriptor at `OFF_EXEC`: the phase in, the exit out.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Exec {
    pub region_id: u64,
    pub mode: u32,
    pub flags: u32,
    pub code_addr: u64,
    pub thunk_addr: u64,
    pub thunk_len: u64,
    pub region_lo: u64,
    pub region_hi: u64,
    pub entry: u64,
    pub code_pages: u32,
    pub nranges: u32,
    pub threads: u32,
    pub ind_reg: u32,
    pub bound_reg: u32,
    pub pad: u32,
    pub step: i64,
    pub ind_start: u64,
    pub ind_end: u64,
    pub loop_exit: u64,
    pub reserved: [u64; 4],
    pub exit_rip: u64,
    pub fault_addr: u64,
    pub exit_kind: u32,
    pub exit_thread: u32,
    pub chunks: u32,
    pub dirty: u32,
    pub faults: u32,
    pub threads_ran: u32,
    pub fetch_ns: u64,
    pub wb_ns: u64,
    pub run_ns: u64,
    pub reserved2: [u64; 5],
    pub regs: Regs,
    pub code_page: [u64; EXEC_MAX_PAGES],
    pub ranges: [Range; EXEC_MAX_RANGES],
}

impl Default for Exec {
    fn default() -> Self {
        Exec {
            region_id: 0,
            mode: 0,
            flags: 0,
            code_addr: 0,
            thunk_addr: 0,
            thunk_len: 0,
            region_lo: 0,
            region_hi: 0,
            entry: 0,
            code_pages: 0,
            nranges: 0,
            threads: 1,
            ind_reg: 0,
            bound_reg: 0,
            pad: 0,
            step: 0,
            ind_start: 0,
            ind_end: 0,
            loop_exit: 0,
            reserved: [0; 4],
            exit_rip: 0,
            fault_addr: 0,
            exit_kind: 0,
            exit_thread: 0,
            chunks: 0,
            dirty: 0,
            faults: 0,
            threads_ran: 0,
            fetch_ns: 0,
            wb_ns: 0,
            run_ns: 0,
            reserved2: [0; 5],
            regs: Regs::default(),
            code_page: [0; EXEC_MAX_PAGES],
            ranges: [Range::default(); EXEC_MAX_RANGES],
        }
    }
}

pub const MAIL_FETCH: u32 = 0;
pub const MAIL_WRITEBACK: u32 = 1;

/// The mailbox at `OFF_MAIL`: the card writes addr, len, kind, then seq;
/// the host serves, writes status, then ack = seq.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Mail {
    pub seq: u64,
    pub addr: u64,
    pub len: u64,
    pub kind: u32,
    pub slot: u32,
    pub ack: u64,
    pub status: i32,
    pub pad2: u32,
}

/// A page written back by the card (`MAIL_WRITEBACK`): only the 64-byte
/// lines whose bit is set in `lines` changed. The table of `WB_TABLE`
/// bytes leads the slot, the pages follow in table order.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct WbPage {
    pub addr: u64,
    pub lines: u64,
}
pub const WB_TABLE: u64 = 64 << 10;
pub const WB_MAX_PAGES: u64 = (EXEC_CHUNK - WB_TABLE) / 4096;

const _: [(); 2256] = [(); std::mem::size_of::<Regs>()];
const _: [(); 24] = [(); std::mem::size_of::<Range>()];
const _: [(); 256 + 2256 + 512 + 1536] = [(); std::mem::size_of::<Exec>()];
const _: [(); 48] = [(); std::mem::size_of::<Mail>()];
const _: [(); 16] = [(); std::mem::size_of::<WbPage>()];
