//! Open the card and perform register-level operations.

use std::thread::sleep;
use std::time::{Duration, Instant};

use phi_regs::postcode::Postcode;
use phi_regs::sbox::{self, DownloadInfo};
use phi_vfio::mapping::Mapping;
use phi_vfio::VfioPci;

use crate::{Error, Result};

/// Bytes written into the aperture between read-backs (see
/// [`Card::write_card_memory_paced`]).
pub const APERTURE_WRITE_CHUNK: usize = 4096;

/// An opened Xeon Phi.
pub struct Card {
    vfio: VfioPci,
    mmio: Mapping,
    aperture: Mapping,
}

/// What [`Card::reset`] observed.
#[derive(Debug, Clone)]
pub struct ResetReport {
    /// Total time from the RGCR write to the ready flag.
    pub took: Duration,
    /// `SPAD2` before the reset.
    pub spad2_before: u32,
    /// `SPAD2` after the bootstrap reported ready.
    pub spad2_after: u32,
    /// Every distinct POST code seen, with the time it first appeared.
    pub trace: Vec<(Duration, Postcode)>,
}

/// Snapshot of the registers `phictl info` prints.
#[derive(Debug, Clone)]
pub struct Status {
    /// PCI address.
    pub bdf: String,
    /// Vendor, device, subsystem vendor, subsystem device.
    pub ids: (u16, u16, u16, u16),
    /// PCI COMMAND register after enabling memory decode.
    pub command: u16,
    /// Raw POST code register.
    pub postcode: Postcode,
    /// Scratchpads 0..16.
    pub spads: [u32; 16],
    /// Decoded scratchpad 2.
    pub download: DownloadInfo,
    /// Aperture BAR size in bytes.
    pub aperture_len: usize,
    /// Decoded scratchpad 4 (platform word).
    pub platform: sbox::PlatformInfo,
    /// Raw and decoded current clock ratio register.
    pub clock_ratio: sbox::ClockRatio,
}

impl Card {
    /// Open the card at `bdf` through VFIO and map both BARs.
    pub fn open(bdf: &str) -> Result<Self> {
        phi_vfio::sysfs::require_phi(bdf)?;
        let vfio = VfioPci::open(bdf)?;
        let cmd = vfio.device().enable_memory_and_bus_master()?;
        log::debug!("PCI COMMAND now {cmd:#06x}");
        let mmio = vfio.device().map_region(sbox::MMIO_BAR_INDEX)?;
        let aperture = vfio.device().map_region(sbox::APER_BAR_INDEX)?;
        log::info!("mapped MMIO {} bytes, aperture {} bytes", mmio.len(), aperture.len());
        Ok(Self { vfio, mmio, aperture })
    }

    /// The underlying VFIO handle.
    pub fn vfio(&self) -> &VfioPci {
        &self.vfio
    }

    /// The aperture mapping (card memory).
    pub fn aperture(&self) -> &Mapping {
        &self.aperture
    }

    /// Pin `len` bytes at `vaddr` and map them for the card at IOMMU address
    /// `iova` (VFIO type1). See [`crate::dma::HostDmaBuffer`].
    ///
    /// # Safety
    /// `vaddr..vaddr+len` must be a page-aligned mapping owned by the caller
    /// that outlives the DMA mapping; the card may write to it.
    pub unsafe fn map_dma(&self, vaddr: *mut u8, iova: u64, len: u64) -> Result<()> {
        Ok(self.vfio.container().map_dma(vaddr, iova, len)?)
    }

    /// Read an SBOX register (offset relative to the SBOX base).
    pub fn sbox_read(&self, off: u32) -> u32 {
        self.mmio.read32((sbox::SBOX_BASE + off) as usize)
    }

    /// Write an SBOX register.
    pub fn sbox_write(&self, off: u32, v: u32) {
        self.mmio.write32((sbox::SBOX_BASE + off) as usize, v)
    }

    /// Current POST code.
    pub fn postcode(&self) -> Postcode {
        Postcode(self.mmio.read32(sbox::POSTCODE as usize))
    }

    /// Scratchpad `n`.
    pub fn spad(&self, n: u32) -> u32 {
        self.sbox_read(sbox::spad(n))
    }

    /// Write scratchpad `n`.
    pub fn set_spad(&self, n: u32, v: u32) {
        self.sbox_write(sbox::spad(n), v)
    }

    /// Decoded scratchpad 2.
    pub fn download_info(&self) -> DownloadInfo {
        DownloadInfo(self.spad(sbox::SPAD_DOWNLOAD_INFO))
    }

    /// Everything `phictl info` shows.
    pub fn status(&self) -> Result<Status> {
        let mut spads = [0u32; 16];
        for (i, s) in spads.iter_mut().enumerate() {
            *s = self.spad(i as u32);
        }
        Ok(Status {
            bdf: self.vfio.bdf().to_string(),
            ids: self.vfio.device().ids()?,
            command: self.vfio.device().read_config_u16(phi_vfio::device::pci_cfg::COMMAND)?,
            postcode: self.postcode(),
            spads,
            download: DownloadInfo(spads[sbox::SPAD_DOWNLOAD_INFO as usize]),
            aperture_len: self.aperture.len(),
            platform: sbox::PlatformInfo(spads[sbox::SPAD_PLATFORM_INFO as usize]),
            clock_ratio: sbox::ClockRatio(self.sbox_read(sbox::CURRENT_CLK_RATIO)),
        })
    }

    /// Reset the card to its bootstrap: set `RGCR` bit 0, wait one second
    /// (Intel: "we really want to delay at least 1 second after touching
    /// reset"), clear `SPAD2`, then wait up to `timeout` for the bootstrap
    /// to report ready again. The POST code register is sampled every 10 ms
    /// throughout and every change is recorded in the report.
    pub fn reset(&self, timeout: Duration) -> Result<ResetReport> {
        let start = Instant::now();
        let spad2_before = self.spad(sbox::SPAD_DOWNLOAD_INFO);
        let mut trace: Vec<(Duration, Postcode)> = vec![(Duration::ZERO, self.postcode())];
        // Read back once so any earlier posted writes have landed.
        let _ = self.sbox_read(sbox::RGCR);
        let rgcr = self.sbox_read(sbox::RGCR);
        self.sbox_write(sbox::RGCR, rgcr | sbox::RGCR_RESET);
        let settle = start + Duration::from_secs(1);
        while Instant::now() < settle {
            self.sample_postcode(start, &mut trace);
            sleep(Duration::from_millis(10));
        }
        // Intel cleared the ready bit after reset so that a stale value could
        // not be mistaken for the bootstrap's fresh announcement.
        self.set_spad(sbox::SPAD_DOWNLOAD_INFO, 0);
        let deadline = start + timeout;
        loop {
            self.sample_postcode(start, &mut trace);
            let d = self.download_info();
            if d.ready() {
                log::info!(
                    "bootstrap ready: download addr {:#x}, BSP APIC id {}",
                    d.download_addr(),
                    d.apic_id()
                );
                break;
            }
            if Instant::now() >= deadline {
                return Err(Error::NotReady(self.postcode().text(), d.0));
            }
            sleep(Duration::from_millis(10));
        }
        Ok(ResetReport {
            took: start.elapsed(),
            spad2_before,
            spad2_after: self.spad(sbox::SPAD_DOWNLOAD_INFO),
            trace,
        })
    }

    fn sample_postcode(&self, start: Instant, trace: &mut Vec<(Duration, Postcode)>) {
        let p = self.postcode();
        if trace.last().map(|(_, last)| *last) != Some(p) {
            trace.push((start.elapsed(), p));
        }
    }

    /// Poll until the bootstrap reports ready (`SPAD2` bit 0) or `timeout`.
    pub fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut last = self.postcode();
        log::info!("bootstrap at POST \"{}\"; waiting for \"12\"", last.text());
        loop {
            let post = self.postcode();
            if post != last {
                log::info!("POST \"{}\" {}", post.text(), post.describe().unwrap_or(""));
                last = post;
            }
            let d = self.download_info();
            // SPAD2's ready bit survives the function reset that VFIO issues at
            // open and is stale until the bootstrap reaches "12" again; the
            // POST code is the authority. Measured 2026-09-14: aperture writes
            // issued on the stale bit, during GDDR training, reset the host.
            if post.is_ready() && d.ready() {
                log::info!(
                    "bootstrap ready: download addr {:#x}, BSP APIC id {}",
                    d.download_addr(),
                    d.apic_id()
                );
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::NotReady(self.postcode().text(), d.0));
            }
            sleep(Duration::from_millis(50));
        }
    }

    /// Send interrupt `vector` to `apic_id` through SBOX ICR `n`. Writes the
    /// destination (high dword) first, reads it back to order the posted
    /// write, then writes the low dword with the send bit.
    pub fn send_icr(&self, n: u32, apic_id: u32, vector: u32) {
        let icr = sbox::apicicr(n);
        self.sbox_write(icr + 4, apic_id);
        let _ = self.sbox_read(icr + 4);
        self.sbox_write(icr, (vector & 0xff) | sbox::ICR_SEND);
        let _ = self.sbox_read(icr);
    }

    /// The boot interrupt: vector 229 to the BSP through ICR 7, as
    /// `mic_x100_send_firmware_intr` did.
    pub fn send_boot_interrupt(&self) {
        let apic = self.download_info().apic_id();
        self.send_icr(7, apic, sbox::BSP_INTERRUPT_VECTOR);
    }

    /// Copy `data` into card memory at card physical `addr` through the aperture.
    pub fn write_card_memory(&self, addr: u64, data: &[u8]) -> Result<()> {
        let end = addr
            .checked_add(data.len() as u64)
            .ok_or_else(|| Error::Range("address overflow".into()))?;
        if end > self.aperture.len() as u64 {
            return Err(Error::Range(format!(
                "write {addr:#x}+{:#x} exceeds aperture {:#x}",
                data.len(),
                self.aperture.len()
            )));
        }
        if end > phi_regs::memory::GDDR_BYTES_3120A {
            return Err(Error::Range(format!(
                "write {addr:#x}+{:#x} exceeds card GDDR ({:#x})",
                data.len(),
                phi_regs::memory::GDDR_BYTES_3120A
            )));
        }
        self.write_card_memory_paced(addr, data, APERTURE_WRITE_CHUNK, true)
    }

    /// The aperture write path with its pacing exposed (used by `phictl fill`).
    ///
    /// Posted writes are issued in chunks of `chunk` bytes; after each chunk,
    /// when `readback` is set, the last eight bytes are read back. That
    /// non-posted read does not complete until the card has accepted every
    /// preceding posted write, which bounds the writes in flight toward a
    /// Gen2 x8 endpoint. Measured 2026-09-14: one 8-byte write at 64 MiB or at
    /// 256 MiB is fine; an unpaced 1 MiB burst of 8-byte writes reset the host
    /// with nothing logged, three times running.
    pub fn write_card_memory_paced(&self, addr: u64, data: &[u8], chunk: usize, readback: bool) -> Result<()> {
        let end = addr
            .checked_add(data.len() as u64)
            .ok_or_else(|| Error::Range("address overflow".into()))?;
        if end > self.aperture.len() as u64 {
            return Err(Error::Range(format!("write {addr:#x}+{:#x} exceeds aperture", data.len())));
        }
        let chunk = chunk.max(8);
        let mut off = 0usize;
        while off < data.len() {
            let n = chunk.min(data.len() - off);
            self.aperture.write_bytes(addr as usize + off, &data[off..off + n]);
            if readback {
                let stop = addr as usize + off + n;
                let start = stop.saturating_sub(8).max(addr as usize);
                let mut probe = [0u8; 8];
                self.aperture.read_bytes(start, &mut probe[..stop - start]);
                let want = &data[start - addr as usize..stop - addr as usize];
                if &probe[..stop - start] != want {
                    return Err(Error::Range(format!(
                        "aperture read-back mismatch at {start:#x}: wrote {want:02x?}, read {:02x?}",
                        &probe[..stop - start]
                    )));
                }
            }
            off += n;
        }
        Ok(())
    }

    /// Read `buf.len()` bytes of card memory at card physical `addr`.
    pub fn read_card_memory(&self, addr: u64, buf: &mut [u8]) -> Result<()> {
        let end = addr
            .checked_add(buf.len() as u64)
            .ok_or_else(|| Error::Range("address overflow".into()))?;
        if end > self.aperture.len() as u64 {
            return Err(Error::Range(format!("read {addr:#x}+{:#x} exceeds aperture", buf.len())));
        }
        self.aperture.read_bytes(addr as usize, buf);
        Ok(())
    }
}
