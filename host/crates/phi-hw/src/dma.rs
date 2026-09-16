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

use std::ptr;
use std::time::{Duration, Instant};

use phi_regs::{memory, sbox};

use crate::card::Card;
use crate::{Error, Result};

/// Number of descriptors in a channel ring (16 bytes each, 4 KiB total;
/// the ring must be aligned to its size, a page here).
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
            return Err(Error::Range(format!("mmap of {len:#x} bytes for DMA failed")));
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

/// A host-owned DMA channel with its descriptor ring in host memory.
pub struct DmaChannel<'a> {
    card: &'a Card,
    chan: u32,
    ring: HostDmaBuffer,
    head: u32,
    /// Descriptors completed by the engine, for statistics.
    pub completed: u64,
}

impl<'a> DmaChannel<'a> {
    /// Take channel `chan` for the host, with a 4 KiB descriptor ring pinned
    /// at IOMMU address `ring_iova`. Programs SMPT entries 0..3 identity.
    pub fn new(card: &'a Card, chan: u32, ring_iova: u64) -> Result<Self> {
        if chan >= sbox::DMA_CHAN_COUNT {
            return Err(Error::Range(format!("DMA channel {chan} does not exist")));
        }
        smpt_identity(card, 4);
        let ring = HostDmaBuffer::new(card, ring_iova, 4096)?;
        let addr = ring.card_addr();
        // Owner host, disabled while the ring is set.
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
        let tail = card.sbox_read(sbox::dma_reg(chan, sbox::DTPR)) % RING_DESCRIPTORS;
        card.sbox_write(sbox::dma_reg(chan, sbox::DHPR), tail);
        card.sbox_write(sbox::DCR, dcr | (3 << (2 * chan)));
        log::info!(
            "DMA channel {chan}: ring at card {addr:#x} ({} descriptors), tail {tail}, DCR {:#x}",
            RING_DESCRIPTORS,
            card.sbox_read(sbox::DCR)
        );
        Ok(Self {
            card,
            chan,
            ring,
            head: tail,
            completed: 0,
        })
    }

    /// The channel number.
    pub fn channel(&self) -> u32 {
        self.chan
    }

    /// Copy `len` bytes from card address `src` to card address `dst`
    /// (either may be host memory through the SMPT window) and wait for the
    /// engine to finish. Addresses and length must be multiples of 64; the
    /// length at most 1 MiB minus 64.
    pub fn copy(&mut self, src: u64, dst: u64, len: usize) -> Result<()> {
        if len == 0 || !len.is_multiple_of(64) || len >= 1 << 20 || !src.is_multiple_of(64) || !dst.is_multiple_of(64) {
            return Err(Error::Range(format!(
                "DMA copy {src:#x} -> {dst:#x}, {len:#x} bytes: not 64-byte granular"
            )));
        }
        let (q0, q1) = sbox::dma_memcpy_desc(src, dst, len as u64);
        let slot = (self.head % RING_DESCRIPTORS) as usize * 16;
        let base = self.ring.as_mut_slice().as_mut_ptr();
        // SAFETY: slot + 16 <= 4096, inside the ring buffer.
        unsafe {
            ptr::write_volatile(base.add(slot) as *mut u64, q0);
            ptr::write_volatile(base.add(slot + 8) as *mut u64, q1);
        }
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        self.head = (self.head + 1) % RING_DESCRIPTORS;
        self.card.sbox_write(sbox::dma_reg(self.chan, sbox::DHPR), self.head);
        let start = Instant::now();
        loop {
            let tail = self.card.sbox_read(sbox::dma_reg(self.chan, sbox::DTPR)) % RING_DESCRIPTORS;
            if tail == self.head {
                self.completed += 1;
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(2) {
                let err = self.card.sbox_read(sbox::dma_reg(self.chan, sbox::DCHERR));
                let stat = self.card.sbox_read(sbox::dma_reg(self.chan, sbox::DSTAT));
                return Err(Error::Range(format!(
                    "DMA channel {} timed out: head {} tail {tail}, DCHERR {err:#x}, DSTAT {stat:#x}",
                    self.chan, self.head
                )));
            }
            std::hint::spin_loop();
        }
    }
}

impl Drop for DmaChannel<'_> {
    fn drop(&mut self) {
        let dcr = self.card.sbox_read(sbox::DCR);
        self.card.sbox_write(sbox::DCR, dcr & !(2 << (2 * self.chan)));
    }
}
