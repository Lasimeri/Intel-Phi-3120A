//! `phictl boot --disk PATH`: serve a disk image to the card over the ring's
//! block channel (kind 4), the host end of the card's `/dev/phiblk0`
//! (kernel patch 0025). See disk.md and docs/spec/ring-protocol.md.
//!
//! The card posts 32-byte request records in the card-to-host ring: a tag,
//! an operation, a byte length, a start sector and the card physical
//! address of the buffer. This thread moves the bytes between the image
//! file and card memory through the aperture (the paced write path of
//! phi-hw) and answers every record with an 8-byte completion in the
//! host-to-card ring. The identify operation returns the image size in
//! sectors, which is how the card learns the capacity. Requests are served
//! in order; the image lives in the host's page cache and a flush request
//! becomes fdatasync, so the card filesystem's barriers hold across a host
//! crash the way a host filesystem's would.
use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use phi_hw::dma::{DmaChannel, HostDmaBuffer};
use phi_hw::ringmem::ApertureRegion;
use phi_hw::Card;
use phi_ring::layout::ring_hdr;
use phi_ring::{ChannelKind, Region, RingMemory};

use crate::serve::wait_for_init;

/// Request record: tag u32, op u32, len u32, reserved u32, sector u64, phys u64.
pub const REQ_SIZE: usize = 32;
/// Completion record: tag u32, status u32 (0 = ok, else an errno; the
/// capacity in sectors for identify).
pub const CPL_SIZE: usize = 8;
pub const OP_READ: u32 = 0;
pub const OP_WRITE: u32 = 1;
pub const OP_FLUSH: u32 = 2;
pub const OP_IDENT: u32 = 3;
/// Tag of the identify record; the answer carries this tag when the host
/// copies through the aperture (the card then bounces through its slots)...
pub const IDENT_TAG: u32 = 0xffff_ffff;
/// ...and this one when the host's path is coherent with the card's caches
/// (the DMA engine), so the card hands its pages over directly.
pub const IDENT_TAG_DIRECT: u32 = 0xffff_fffe;
/// Bytes per sector.
pub const SECTOR: u64 = 512;
/// Largest data length in one record (the card driver's slot size).
pub const MAX_LEN: usize = 524_288;

/// IOMMU addresses of the DMA descriptor rings and staging buffers, one
/// set per DMA channel (any unused range below the first SMPT page will do).
const DMA_RING_IOVA: u64 = 0x1000_0000;
const DMA_DATA_IOVA: u64 = 0x1010_0000;
/// IOMMU address of the host memory window (`--host-mem`), above the DMA
/// rings and below the IOMMU's reserved ranges; up to 60 GiB fits.
pub const HOSTMEM_IOVA: u64 = 0x1_0000_0000;
/// Scratch card memory for the DMA self-test: the last 2 MiB of the 16 MiB
/// ring region, which no channel uses; 1 MiB per DMA channel, minus the
/// last 4 KiB, which hold the channels' status words.
const SELFTEST_OFFSET: u64 = 0xE0_0000;
const STATUS_OFFSET: u64 = 0xFF_F000;

/// What a block channel is backed by.
pub enum Backend {
    /// A disk image file.
    File(File),
    /// Pinned host memory (the `--host-mem` window): the card's swap or scratch.
    Ram(HostDmaBuffer),
}

const EIO: u32 = 5;
const EINVAL: u32 = 22;
const EOPNOTSUPP: u32 = 95;

/// One decoded request record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub tag: u32,
    pub op: u32,
    pub len: u32,
    pub sector: u64,
    pub phys: u64,
}

impl Request {
    /// Decode a record.
    pub fn parse(rec: &[u8; REQ_SIZE]) -> Self {
        let u32_at = |o: usize| u32::from_le_bytes(rec[o..o + 4].try_into().unwrap());
        let u64_at = |o: usize| u64::from_le_bytes(rec[o..o + 8].try_into().unwrap());
        Request {
            tag: u32_at(0),
            op: u32_at(4),
            len: u32_at(8),
            sector: u64_at(16),
            phys: u64_at(24),
        }
    }
}

/// Encode a completion record.
pub fn completion(tag: u32, status: u32) -> [u8; CPL_SIZE] {
    let mut c = [0u8; CPL_SIZE];
    c[..4].copy_from_slice(&tag.to_le_bytes());
    c[4..].copy_from_slice(&status.to_le_bytes());
    c
}

/// Counters printed when the service stops.
#[derive(Default, Debug)]
pub struct Stats {
    pub reads: u64,
    pub writes: u64,
    pub flushes: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub errors: u64,
}

/// Serve one request against the image; returns the completion status.
fn serve_one(
    card: &Card,
    path: &mut Path<'_>,
    backend: &mut Backend,
    capacity: u64,
    r: &Request,
    buf: &mut [u8],
    stats: &mut Stats,
) -> u32 {
    match r.op {
        OP_IDENT => capacity.min(u64::from(u32::MAX)) as u32,
        OP_FLUSH => {
            stats.flushes += 1;
            match backend {
                Backend::File(file) => match file.sync_data() {
                    Ok(()) => 0,
                    Err(_) => EIO,
                },
                Backend::Ram(_) => 0,
            }
        }
        OP_READ | OP_WRITE => {
            let len = r.len as usize;
            if len == 0 || len > MAX_LEN || !(len as u64).is_multiple_of(SECTOR) || r.sector + len as u64 / SECTOR > capacity {
                stats.errors += 1;
                return EINVAL;
            }
            let off = r.sector * SECTOR;
            if r.op == OP_READ {
                stats.reads += 1;
                stats.bytes_read += len as u64;
            } else {
                stats.writes += 1;
                stats.bytes_written += len as u64;
            }
            let res = match (r.op, path, backend) {
                (OP_READ, Path::Dma { chan, buf: dbuf }, Backend::File(file)) => file
                    .read_exact_at(&mut dbuf.as_mut_slice()[..len], off)
                    .map_err(anyhow::Error::from)
                    .and_then(|()| chan.copy(dbuf.card_addr(), r.phys, len).map_err(anyhow::Error::from)),
                (OP_READ, Path::Aperture, Backend::File(file)) => file
                    .read_exact_at(&mut buf[..len], off)
                    .map_err(anyhow::Error::from)
                    .and_then(|()| card.write_card_memory(r.phys, &buf[..len]).map_err(anyhow::Error::from)),
                (_, Path::Dma { chan, buf: dbuf }, Backend::File(file)) => chan
                    .copy(r.phys, dbuf.card_addr(), len)
                    .map_err(anyhow::Error::from)
                    .and_then(|()| file.write_all_at(&dbuf.as_slice()[..len], off).map_err(anyhow::Error::from)),
                (_, Path::Aperture, Backend::File(file)) => card
                    .read_card_memory(r.phys, &mut buf[..len])
                    .map_err(anyhow::Error::from)
                    .and_then(|()| file.write_all_at(&buf[..len], off).map_err(anyhow::Error::from)),
                // Host memory: the DMA engine moves the bytes straight between the
                // window and the card's page; no staging at all.
                (OP_READ, Path::Dma { chan, .. }, Backend::Ram(ram)) => {
                    chan.copy(ram.card_addr() + off, r.phys, len).map_err(anyhow::Error::from)
                }
                (_, Path::Dma { chan, .. }, Backend::Ram(ram)) => {
                    chan.copy(r.phys, ram.card_addr() + off, len).map_err(anyhow::Error::from)
                }
                (OP_READ, Path::Aperture, Backend::Ram(ram)) => {
                    let o = off as usize;
                    card.write_card_memory(r.phys, &ram.as_slice()[o..o + len])
                        .map_err(anyhow::Error::from)
                }
                (_, Path::Aperture, Backend::Ram(ram)) => {
                    let o = off as usize;
                    card.read_card_memory(r.phys, &mut ram.as_mut_slice()[o..o + len])
                        .map_err(anyhow::Error::from)
                }
            };
            match res {
                Ok(()) => 0,
                Err(e) => {
                    stats.errors += 1;
                    eprintln!(
                        "[phictl] disk: {} at sector {} ({} bytes): {e:#}",
                        if r.op == OP_READ { "read" } else { "write" },
                        r.sector,
                        len
                    );
                    EIO
                }
            }
        }
        _ => EOPNOTSUPP,
    }
}

/// Serve `path` on the block channel until the process ends.
/// Open a disk image as a backend.
pub fn open_image(path: &std::path::Path) -> Result<Backend> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("open disk image {}", path.display()))?;
    Ok(Backend::File(file))
}

/// Serve `backend` on the channel of `kind` (DMA channel `chan_no`) until the process ends.
#[allow(clippy::too_many_arguments)]
pub fn run(
    card: &Card,
    ring_base: u64,
    ring_size: u64,
    mut backend: Backend,
    what: &str,
    kind: ChannelKind,
    chan_no: u32,
    dma: bool,
) -> Result<()> {
    let capacity = match &backend {
        Backend::File(file) => file.metadata()?.len() / SECTOR,
        Backend::Ram(ram) => ram.len() as u64 / SECTOR,
    };
    if !wait_for_init(card, Duration::from_secs(120)) {
        eprintln!("[phictl] disk: the card did not reach init; not serving");
        return Ok(());
    }
    let mut mem = ApertureRegion::new(card.aperture(), ring_base as usize, ring_size as usize);
    let (producer, consumer, consumer_base, ring_capacity) = loop {
        if let Ok(region) = Region::open(&mem) {
            if let (Ok(endpoints), Ok(view)) = (region.host_endpoints(&mem, kind), region.channel(kind)) {
                let cap = endpoints.1.capacity();
                break (endpoints.0, endpoints.1, view.c2h_offset, cap);
            }
        }
        thread::sleep(Duration::from_millis(200));
    };
    eprintln!(
        "[phictl] disk: serving {what} ({} MiB) on channel kind {}",
        (capacity * SECTOR) >> 20,
        kind as u32
    );
    let mut data_path = if dma { open_path(card, ring_base, chan_no) } else { Path::Aperture };
    let mut buf = vec![0u8; MAX_LEN];
    let mut rec = [0u8; REQ_SIZE];
    let mut stats = Stats::default();
    let mut trace: u32 = if std::env::var_os("PHICTL_DISK_TRACE").is_some() { 40 } else { 0 };
    let mut last_active = Instant::now();
    let mut anomalies: u64 = 0;
    loop {
        // The indices are handled here rather than by the generic consumer:
        // its resynchronisation on an impossible count discards records, and
        // a discarded block request hangs the card. An impossible count is
        // logged and re-read instead.
        let head = mem.read_u32(consumer_base + ring_hdr::HEAD);
        let tail = mem.read_u32(consumer_base + ring_hdr::TAIL);
        let avail = head.wrapping_sub(tail);
        if avail > ring_capacity {
            anomalies += 1;
            if anomalies <= 5 || anomalies.is_power_of_two() {
                eprintln!(
                    "[phictl] disk: {what}: impossible request count (head {head:#x} tail {tail:#x}); re-reading (anomaly {anomalies})"
                );
            }
            thread::sleep(Duration::from_micros(200));
            continue;
        }
        if (avail as usize) < REQ_SIZE {
            // Spin briefly after activity (a request in flight usually has
            // company), then back off to a short sleep.
            if last_active.elapsed() < Duration::from_millis(3) {
                std::hint::spin_loop();
            } else {
                thread::sleep(Duration::from_micros(200));
            }
            continue;
        }
        last_active = Instant::now();
        let n = consumer.pop(&mut mem, &mut rec);
        if n != REQ_SIZE {
            anomalies += 1;
            eprintln!("[phictl] disk: {what}: popped {n} bytes instead of a record (head {head:#x} tail {tail:#x}); anomaly {anomalies}");
            continue;
        }
        let r = Request::parse(&rec);
        if trace > 0 {
            trace -= 1;
            eprintln!(
                "[phictl] disk: rec {n} bytes tag {:#x} op {} len {} sector {} phys {:#x}",
                r.tag, r.op, r.len, r.sector, r.phys
            );
        }
        let status = serve_one(card, &mut data_path, &mut backend, capacity, &r, &mut buf, &mut stats);
        // The identify answer tells the card whether to hand its pages over
        // (direct) or to bounce through its uncached slots. Both host paths are
        // coherent with the card's caches (measured 2026-09-16, clevtest and
        // clevtest2 in the DMA results), so direct is the default for both;
        // PHICTL_DISK_BOUNCE=1 keeps the bounce path for experiments.
        let tag = if r.op == OP_IDENT {
            if std::env::var_os("PHICTL_DISK_BOUNCE").is_some() {
                IDENT_TAG
            } else {
                IDENT_TAG_DIRECT
            }
        } else {
            r.tag
        };
        let mut waited = 0u32;
        while (producer.free(&mem) as usize) < CPL_SIZE {
            thread::sleep(Duration::from_millis(1));
            waited += 1;
            if waited == 100 || waited.is_multiple_of(10_000) {
                let (h, t) = producer.indices(&mem);
                anomalies += 1;
                eprintln!("[phictl] disk: {what}: completion ring full for {waited} ms (head {h:#x} tail {t:#x}); anomaly {anomalies}");
            }
        }
        producer.push(&mut mem, &completion(tag, status));
    }
}

/// The data path: the DMA engine, or aperture copies when it is unusable.
enum Path<'a> {
    Dma { chan: DmaChannel<'a>, buf: HostDmaBuffer },
    Aperture,
}

/// Bring up the DMA engine and prove it with a round trip through card
/// memory the card does not use: pattern into the card by DMA, read back
/// through the aperture; pattern into the card through the aperture, read
/// back by DMA. Returns the aperture path with the reason when anything
/// fails, so the disk still works.
fn open_path(card: &Card, ring_base: u64, chan_no: u32) -> Path<'_> {
    let mut chan = match DmaChannel::new(
        card,
        chan_no,
        DMA_RING_IOVA + u64::from(chan_no) * 0x2000,
        ring_base + STATUS_OFFSET + u64::from(chan_no) * 64,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[phictl] disk: DMA channel unusable ({e}); copying through the aperture");
            return Path::Aperture;
        }
    };
    let mut buf = match HostDmaBuffer::new(card, DMA_DATA_IOVA + u64::from(chan_no) * 0x10_0000, MAX_LEN) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[phictl] disk: DMA buffer unusable ({e}); copying through the aperture");
            return Path::Aperture;
        }
    };
    let scratch = ring_base + SELFTEST_OFFSET + u64::from(chan_no) * 0x10_0000;
    let n = 65536;
    let pattern: Vec<u8> = (0..n).map(|i| (i as u32).wrapping_mul(2654435761) as u8 ^ (i >> 8) as u8).collect();
    let mut back = vec![0u8; n];
    let test = (|| -> Result<f64> {
        buf.as_mut_slice()[..n].copy_from_slice(&pattern);
        chan.copy(buf.card_addr(), scratch, n)?;
        card.read_card_memory(scratch, &mut back)?;
        if back != pattern {
            anyhow::bail!("host to card DMA delivered wrong data");
        }
        let other: Vec<u8> = pattern.iter().map(|b| !b).collect();
        card.write_card_memory(scratch + n as u64, &other)?;
        buf.as_mut_slice()[..n].fill(0);
        chan.copy(scratch + n as u64, buf.card_addr(), n)?;
        if buf.as_slice()[..n] != other[..] {
            anyhow::bail!("card to host DMA delivered wrong data");
        }
        // Optional stress: random lengths and offsets, verified, when
        // PHICTL_DMA_STRESS=N is set (a self-test of the engine, no filesystem).
        if let Some(n) = std::env::var("PHICTL_DMA_STRESS").ok().and_then(|v| v.parse::<u32>().ok()) {
            let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
            let mut rnd = || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed
            };
            let half = MAX_LEN / 2;
            let mut failures = 0u32;
            for it in 0..n {
                let len = 64 * (1 + (rnd() as usize % (half / 64)));
                let off = 64 * (rnd() as usize % ((half - len) / 64 + 1));
                let coff = 64 * (rnd() as usize % ((half - len) / 64 + 1));
                let tag = rnd() as u8;
                for (i, b) in buf.as_mut_slice()[off..off + len].iter_mut().enumerate() {
                    *b = tag ^ (i as u8) ^ ((i >> 8) as u8);
                }
                buf.as_mut_slice()[half..].fill(0);
                chan.copy(buf.card_addr() + off as u64, scratch + coff as u64, len)?;
                chan.copy(scratch + coff as u64, buf.card_addr() + half as u64, len)?;
                let (a, b) = buf.as_slice().split_at(half);
                if a[off..off + len] != b[..len] {
                    failures += 1;
                    let first = a[off..off + len].iter().zip(&b[..len]).position(|(x, y)| x != y).unwrap_or(0);
                    let zeros = b[..len].iter().all(|&x| x == 0);
                    // Replay the same shape three times to tell a shape from a race.
                    let mut again = 0;
                    for _ in 0..3 {
                        buf.as_mut_slice()[half..].fill(0);
                        chan.copy(buf.card_addr() + off as u64, scratch + coff as u64, len)?;
                        chan.copy(scratch + coff as u64, buf.card_addr() + half as u64, len)?;
                        let (a2, b2) = buf.as_slice().split_at(half);
                        if a2[off..off + len] != b2[..len] {
                            again += 1;
                        }
                    }
                    if failures <= 8 {
                        eprintln!("[phictl] disk: DMA stress {it}: len {len:#x} host off {off:#x} card off {coff:#x}: first mismatch at byte {first:#x}, copied back {}; replay failed {again} of 3", if zeros { "all zeros" } else { "other data" });
                    }
                }
            }
            eprintln!("[phictl] disk: DMA stress: {n} random copies, {failures} failures");
            if failures > 0 {
                anyhow::bail!("DMA stress failed {failures} times");
            }
        }
        // Throughput of 16 round trips of MAX_LEN through the scratch area.
        let t = Instant::now();
        for _ in 0..16 {
            chan.copy(buf.card_addr(), scratch, MAX_LEN)?;
            chan.copy(scratch, buf.card_addr(), MAX_LEN)?;
        }
        Ok(32.0 * MAX_LEN as f64 / t.elapsed().as_secs_f64() / 1e6)
    })();
    match test {
        Ok(rate) => {
            eprintln!(
                "[phictl] disk: DMA engine channel {} verified, {rate:.0} MB/s in the self-test",
                chan.channel()
            );
            Path::Dma { chan, buf }
        }
        Err(e) => {
            eprintln!("[phictl] disk: DMA self-test failed ({e:#}); copying through the aperture");
            Path::Aperture
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let mut rec = [0u8; REQ_SIZE];
        rec[..4].copy_from_slice(&0x0102u32.to_le_bytes());
        rec[4..8].copy_from_slice(&OP_WRITE.to_le_bytes());
        rec[8..12].copy_from_slice(&4096u32.to_le_bytes());
        rec[16..24].copy_from_slice(&123456u64.to_le_bytes());
        rec[24..32].copy_from_slice(&0x1234_5000u64.to_le_bytes());
        let r = Request::parse(&rec);
        assert_eq!(
            r,
            Request {
                tag: 0x0102,
                op: OP_WRITE,
                len: 4096,
                sector: 123456,
                phys: 0x1234_5000
            }
        );
        assert_eq!(completion(7, 0), [7, 0, 0, 0, 0, 0, 0, 0]);
    }
}
