//! The SBOX DMA engine, driven from the host.
//!
//! One of the eight channels is taken as host owned: its descriptor ring
//! lives in host memory, software advances the head pointer (`DHPR`) and
//! the engine advances the tail (`DTPR`) as it completes descriptors
//! (SSDG 328207-002, 2.1.8.2.1). Host memory is reachable by the card at
//! `SMPT_BASE + iova` once the system memory page table maps its 16 GiB
//! pages identity onto IOMMU addresses, and IOMMU addresses come from VFIO
//! (`Card::map_dma`). Transfers are cache-line granular (64 bytes) and go
//! host to card, card to host or card to card; each descriptor moves up to
//! 1 MiB. Completion is observed by polling `DTPR`; interrupts stay
//! masked. Register layout and bit positions come from MPSS
//! `dma/mic_dma_md.c` and `include/mic/mic_dma_md.h`; see dma.md.

use phi_vfio::traffic;
use std::ptr;
use std::time::{Duration, Instant};

use phi_regs::{memory, sbox};

use crate::card::Card;
use crate::{Error, Result};

/// Number of descriptors in a channel ring (16 bytes each, 4 KiB total;
/// the ring must be aligned to its size, a page here). Submissions use one
/// 64-byte line (four descriptors) each, so 64 copies fit in flight.
pub const RING_DESCRIPTORS: u32 = 256;

/// A pinned host buffer the card can address.
pub struct HostDmaBuffer {
    ptr: *mut u8,
    len: usize,
    iova: u64,
}

// SAFETY: the buffer is plain memory owned by this struct; sharing the
// pointer across threads is fine as long as accesses are coordinated by
// the owner, which the disk service does by using it from one thread.
unsafe impl Send for HostDmaBuffer {}

impl HostDmaBuffer {
    /// Allocate `len` bytes (a multiple of 4096), populate and pin them,
    /// and map them for the card at IOMMU address `iova`.
    pub fn new(card: &Card, iova: u64, len: usize) -> Result<Self> {
        if len == 0 || !len.is_multiple_of(4096) || !iova.is_multiple_of(4096) {
            return Err(Error::Range(format!(
                "DMA buffer {len:#x} bytes at iova {iova:#x} is not page aligned"
            )));
        }
        // SAFETY: anonymous private mapping; checked for MAP_FAILED below.
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_POPULATE,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(Error::Io("mmap of the DMA buffer", std::io::Error::last_os_error()));
        }
        let ptr = ptr as *mut u8;
        // SAFETY: the mapping above is page aligned and lives as long as
        // this struct; VFIO pins it.
        if let Err(e) = unsafe { card.map_dma(ptr, iova, len as u64) } {
            // SAFETY: undo the mapping made above.
            unsafe { libc::munmap(ptr as *mut libc::c_void, len) };
            return Err(e);
        }
        Ok(Self { ptr, len, iova })
    }

    /// The buffer as bytes.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: the mapping is valid for `len` bytes and lives with self.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// The buffer as mutable bytes.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as above, with exclusive access through `&mut self`.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True if empty (never, but clippy asks).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// IOMMU address of the first byte.
    pub fn iova(&self) -> u64 {
        self.iova
    }

    /// Card address of the first byte, given the identity SMPT map.
    pub fn card_addr(&self) -> u64 {
        memory::SMPT_BASE + self.iova
    }
}

impl HostDmaBuffer {
    /// Map `len` bytes of the file at `path` (created or resized, mode 0600,
    /// MAP_SHARED so other host processes can open the same bytes), populate
    /// and pin them, and map them for the card at `iova`. Host memory the
    /// card can use as a block device and through `/dev/phihost`.
    pub fn from_file(card: &Card, iova: u64, path: &std::path::Path, len: usize) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        if len == 0 || !len.is_multiple_of(1 << 21) || !iova.is_multiple_of(1 << 21) {
            return Err(Error::Range(format!(
                "host memory {len:#x} bytes at iova {iova:#x} is not 2 MiB aligned"
            )));
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|e| Error::Io("open the host memory file", e))?;
        file.set_len(len as u64).map_err(|e| Error::Io("size the host memory file", e))?;
        use std::os::fd::AsRawFd;
        // SAFETY: shared mapping of a file we own; checked for MAP_FAILED below.
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(Error::Io("mmap of the host memory file", std::io::Error::last_os_error()));
        }
        let ptr = ptr as *mut u8;
        // SAFETY: page aligned mapping living as long as this struct; VFIO pins it.
        if let Err(e) = unsafe { card.map_dma(ptr, iova, len as u64) } {
            // SAFETY: undo the mapping made above.
            unsafe { libc::munmap(ptr as *mut libc::c_void, len) };
            return Err(e);
        }
        Ok(Self { ptr, len, iova })
    }
}

impl Drop for HostDmaBuffer {
    fn drop(&mut self) {
        // The VFIO mapping keeps the pages pinned until the container closes
        // with the process; the virtual mapping can go.
        // SAFETY: undoing the mmap made in `new`.
        unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
    }
}

/// Map the first `pages` SMPT entries identity: card address
/// `SMPT_BASE + n * 16 GiB` reaches IOMMU address `n * 16 GiB`, snooped
/// (`mic_smpt_init` in MPSS does the same for all 32).
pub fn smpt_identity(card: &Card, pages: u32) {
    for n in 0..pages.min(sbox::SMPT_COUNT) {
        card.sbox_write(sbox::smpt(n), memory::smpt_entry(u64::from(n) << memory::SMPT_PAGE_SHIFT, false));
    }
}

/// Serialises read-modify-write of the shared DCR register between channels
/// set up from different threads.
static DCR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Submissions the ring can hold at once: one 64-byte line (four
/// descriptors) each, minus one line so the head never meets the tail.
pub const MAX_IN_FLIGHT: u64 = (RING_DESCRIPTORS / 4 - 1) as u64;

/// How long a submitted copy may stay incomplete before the channel is
/// declared stuck.
const COPY_TIMEOUT: Duration = Duration::from_secs(2);

/// A host-owned DMA channel with its descriptor ring in host memory.
///
/// Copies are submitted and completed separately, so many can be in the
/// engine at once (`submit`, `poll`, `wait`); `copy` is the pair for a
/// caller that wants one at a time.
pub struct DmaChannel<'a> {
    card: &'a Card,
    chan: u32,
    /// 8 KiB: the descriptor ring in the first page, status words in the second.
    ring: HostDmaBuffer,
    /// Card address of a status word for host-to-card copies.
    status_card: u64,
    head: u32,
    /// Sequence number of the last submitted copy.
    submitted: u64,
    /// Sequence number of the last submitted copy whose status word lives
    /// in card memory (a copy into the card); 0 when none is outstanding.
    pending_to_card: u64,
    /// Copies known complete, also the sequence number written by status
    /// descriptors. Everything up to it is done: the engine completes in order.
    pub completed: u64,
    /// When `completed` last advanced, or the last moment nothing was in flight.
    progress: Instant,
}

impl<'a> DmaChannel<'a> {
    /// Take channel `chan` for the host, with a 4 KiB descriptor ring pinned
    /// at IOMMU address `ring_iova` (8 KiB are pinned: status words follow the
    /// ring) and a 64-byte status slot in card memory at `status_card`.
    /// Programs SMPT entries 0..3 identity.
    pub fn new(card: &'a Card, chan: u32, ring_iova: u64, status_card: u64) -> Result<Self> {
        if !status_card.is_multiple_of(64) {
            return Err(Error::Range(format!("status slot {status_card:#x} is not 64-byte aligned")));
        }
        if chan >= sbox::DMA_CHAN_COUNT {
            return Err(Error::Range(format!("DMA channel {chan} does not exist")));
        }
        smpt_identity(card, 4);
        let mut ring = HostDmaBuffer::new(card, ring_iova, 8192)?;
        let addr = ring.card_addr();
        // The card-side status word outlives this process: a previous daemon
        // left its last sequence number there, which would read as completion
        // of this one's first copies.
        card.write_card_memory(status_card, &[0u8; 8])?;
        // Owner host, disabled while the ring is set. DCR is shared by all channels.
        let guard = DCR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dcr = card.sbox_read(sbox::DCR) & !(3 << (2 * chan));
        card.sbox_write(sbox::DCR, dcr | (1 << (2 * chan)));
        card.sbox_write(sbox::dma_reg(chan, sbox::DRAR_LO), addr as u32);
        let hi = (RING_DESCRIPTORS & 0x1ffff) << 4
            | ((addr >> 32) & 0x3) as u32
            | (((addr >> memory::SMPT_PAGE_SHIFT) & 0x1f) as u32) << 21
            | sbox::DRAR_HI_SYS;
        card.sbox_write(sbox::dma_reg(chan, sbox::DRAR_HI), hi);
        let dcar = card.sbox_read(sbox::dma_reg(chan, sbox::DCAR));
        card.sbox_write(sbox::dma_reg(chan, sbox::DCAR), dcar | sbox::DCAR_IM0 | sbox::DCAR_IM1);
        card.sbox_write(sbox::dma_reg(chan, sbox::DCHERRMSK), 0);
        // Start on a cache line boundary (see `submit`): a fresh channel reports
        // tail 0; anything else is realigned upwards with NOPs.
        let tail = card.sbox_read(sbox::dma_reg(chan, sbox::DTPR)) % RING_DESCRIPTORS;
        let head = tail.div_ceil(4) * 4 % RING_DESCRIPTORS;
        if head != tail {
            let base = ring.as_mut_slice().as_mut_ptr();
            for i in tail..tail + (head + RING_DESCRIPTORS - tail) % RING_DESCRIPTORS {
                // SAFETY: inside the 4 KiB ring.
                unsafe {
                    ptr::write_volatile(base.add((i % RING_DESCRIPTORS) as usize * 16) as *mut u64, 0);
                    ptr::write_volatile(base.add((i % RING_DESCRIPTORS) as usize * 16 + 8) as *mut u64, 0);
                }
            }
            std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        }
        card.sbox_write(sbox::dma_reg(chan, sbox::DHPR), head);
        card.sbox_write(sbox::DCR, dcr | (3 << (2 * chan)));
        drop(guard);
        log::info!(
            "DMA channel {chan}: ring at card {addr:#x} ({} descriptors), tail {tail}, DCR {:#x}",
            RING_DESCRIPTORS,
            card.sbox_read(sbox::DCR)
        );
        let me = Self {
            card,
            chan,
            ring,
            status_card,
            head,
            submitted: 0,
            pending_to_card: 0,
            completed: 0,
            progress: Instant::now(),
        };
        if head != tail {
            me.wait_idle()?;
        }
        Ok(me)
    }

    /// The channel number.
    pub fn channel(&self) -> u32 {
        self.chan
    }

    /// Copies submitted and not yet known complete.
    pub fn in_flight(&self) -> u64 {
        self.submitted - self.completed
    }

    /// Wait until the engine has consumed everything up to the head.
    fn wait_idle(&self) -> Result<()> {
        let start = Instant::now();
        loop {
            let tail = self.card.sbox_read(sbox::dma_reg(self.chan, sbox::DTPR)) % RING_DESCRIPTORS;
            if tail == self.head {
                return Ok(());
            }
            if start.elapsed() > COPY_TIMEOUT {
                return Err(Error::Dma(format!(
                    "channel {} did not drain (head {} tail {tail})",
                    self.chan, self.head
                )));
            }
            std::hint::spin_loop();
        }
    }

    /// The channel's registers, for a stuck-copy message.
    fn describe(&self) -> String {
        let tail = self.card.sbox_read(sbox::dma_reg(self.chan, sbox::DTPR)) % RING_DESCRIPTORS;
        let err = self.card.sbox_read(sbox::dma_reg(self.chan, sbox::DCHERR));
        let stat = self.card.sbox_read(sbox::dma_reg(self.chan, sbox::DSTAT));
        format!(
            "channel {}: head {} tail {tail}, completed {} of {} submitted, DCHERR {err:#x}, DSTAT {stat:#x}",
            self.chan, self.head, self.completed, self.submitted
        )
    }

    /// Submit a copy of `len` bytes from card address `src` to card address
    /// `dst` (either may be host memory through the SMPT window) and return
    /// its sequence number; `wait` or `poll` tell when the data is visible
    /// at the destination. Addresses and length must be multiples of 64;
    /// the length at most 1 MiB minus 64. Blocks only while the ring is
    /// full (`MAX_IN_FLIGHT` copies outstanding).
    ///
    /// Completion is not the tail pointer: the engine advances it before its
    /// writes are visible in memory (measured 2026-09-16: 225 of 10000 rapid
    /// copies read back stale data when the next copy followed the tail).
    /// Each submission is one cache line: the copy, a status descriptor
    /// that writes the sequence number after the copy through the same
    /// path, and two NOPs. The status word lives in host memory for copies
    /// into host memory and in card memory (read back through the aperture)
    /// for copies into the card, so its arrival implies the data's. This is
    /// what MPSS's DMA library does with its poll ring. Sequence numbers
    /// only grow and the engine runs the ring in order, so the larger of the
    /// two words is the last copy done and everything before it is done too.
    pub fn submit(&mut self, src: u64, dst: u64, len: usize) -> Result<u64> {
        if len == 0 || !len.is_multiple_of(64) || len >= 1 << 20 || !src.is_multiple_of(64) || !dst.is_multiple_of(64) {
            return Err(Error::Range(format!(
                "DMA copy {src:#x} -> {dst:#x}, {len:#x} bytes: not 64-byte granular"
            )));
        }
        while self.in_flight() >= MAX_IN_FLIGHT {
            self.poll()?;
            std::hint::spin_loop();
        }
        let to_host = dst >= memory::SMPT_BASE;
        let seq = self.submitted + 1;
        let status_addr = if to_host { self.ring.card_addr() + 4096 } else { self.status_card };
        let (q0, q1) = sbox::dma_memcpy_desc(src, dst, len as u64);
        let (s0, s1) = sbox::dma_status_desc(seq, status_addr);
        debug_assert!(self.head.is_multiple_of(4));
        let slot = (self.head % RING_DESCRIPTORS) as usize * 16;
        let base = self.ring.as_mut_slice().as_mut_ptr();
        // SAFETY: slot + 64 <= 4096 (the ring) lies inside the 8 KiB buffer.
        unsafe {
            ptr::write_volatile(base.add(slot) as *mut u64, q0);
            ptr::write_volatile(base.add(slot + 8) as *mut u64, q1);
            ptr::write_volatile(base.add(slot + 16) as *mut u64, s0);
            ptr::write_volatile(base.add(slot + 24) as *mut u64, s1);
            for nop in 2..4 {
                ptr::write_volatile(base.add(slot + nop * 16) as *mut u64, 0);
                ptr::write_volatile(base.add(slot + nop * 16 + 8) as *mut u64, 0);
            }
        }
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        self.head = (self.head + 4) % RING_DESCRIPTORS;
        self.card.sbox_write(sbox::dma_reg(self.chan, sbox::DHPR), self.head);
        if self.in_flight() == 0 {
            self.progress = Instant::now();
        }
        self.submitted = seq;
        if !to_host {
            self.pending_to_card = seq;
        }
        if to_host {
            traffic::add(&traffic::DMA_FROM_CARD, len);
            traffic::add(&traffic::DMA_COPIES_FROM_CARD, 1);
        } else if src >= memory::SMPT_BASE {
            traffic::add(&traffic::DMA_TO_CARD, len);
            traffic::add(&traffic::DMA_COPIES_TO_CARD, 1);
        }
        Ok(seq)
    }

    /// The sequence number of the last completed copy, read from the status
    /// words (the card's only when a copy into the card is outstanding: that
    /// read is an aperture transaction). Fails when a copy has been
    /// outstanding for `COPY_TIMEOUT`.
    pub fn poll(&mut self) -> Result<u64> {
        if self.in_flight() == 0 {
            self.progress = Instant::now();
            return Ok(self.completed);
        }
        // SAFETY: the status word is inside the pinned buffer; an aligned
        // 8-byte load is atomic against the engine's 8-byte write.
        let mut seen = unsafe { ptr::read_volatile(self.ring.as_slice().as_ptr().add(4096) as *const u64) };
        if self.pending_to_card > seen {
            let mut w = [0u8; 8];
            self.card.read_card_memory(self.status_card, &mut w)?;
            seen = seen.max(u64::from_le_bytes(w));
        }
        if seen > self.submitted {
            return Err(Error::Dma(format!(
                "status word {seen} is ahead of the {} copies submitted; {}",
                self.submitted,
                self.describe()
            )));
        }
        if seen > self.completed {
            self.completed = seen;
            self.progress = Instant::now();
            if self.pending_to_card <= seen {
                self.pending_to_card = 0;
            }
        } else if self.progress.elapsed() > COPY_TIMEOUT {
            return Err(Error::Dma(format!("copy {} timed out; {}", self.completed + 1, self.describe())));
        }
        Ok(self.completed)
    }

    /// Wait until the copy with sequence number `seq` is complete.
    pub fn wait(&mut self, seq: u64) -> Result<()> {
        while self.poll()? < seq {
            std::hint::spin_loop();
        }
        Ok(())
    }

    /// Copy `len` bytes from card address `src` to card address `dst` and
    /// wait until the data is visible at the destination: `submit` then
    /// `wait`, for one copy at a time.
    pub fn copy(&mut self, src: u64, dst: u64, len: usize) -> Result<()> {
        let seq = self.submit(src, dst, len)?;
        self.wait(seq)
    }
}

impl Drop for DmaChannel<'_> {
    fn drop(&mut self) {
        let _guard = DCR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dcr = self.card.sbox_read(sbox::DCR);
        self.card.sbox_write(sbox::DCR, dcr & !(2 << (2 * self.chan)));
    }
}
