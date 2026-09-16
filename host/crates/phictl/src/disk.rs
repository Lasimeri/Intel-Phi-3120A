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
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use phi_hw::ringmem::ApertureRegion;
use phi_hw::Card;
use phi_ring::{ChannelKind, Region};

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
/// Bytes per sector.
pub const SECTOR: u64 = 512;
/// Largest data length in one record (the card driver's slot size).
pub const MAX_LEN: usize = 524_288;

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
fn serve_one(card: &Card, file: &File, capacity: u64, r: &Request, buf: &mut [u8], stats: &mut Stats) -> u32 {
    match r.op {
        OP_IDENT => capacity.min(u64::from(u32::MAX)) as u32,
        OP_FLUSH => {
            stats.flushes += 1;
            match file.sync_data() {
                Ok(()) => 0,
                Err(_) => EIO,
            }
        }
        OP_READ | OP_WRITE => {
            let len = r.len as usize;
            if len == 0 || len > MAX_LEN || !(len as u64).is_multiple_of(SECTOR) || r.sector + len as u64 / SECTOR > capacity {
                stats.errors += 1;
                return EINVAL;
            }
            let off = r.sector * SECTOR;
            let res = if r.op == OP_READ {
                stats.reads += 1;
                stats.bytes_read += len as u64;
                file.read_exact_at(&mut buf[..len], off)
                    .map_err(anyhow::Error::from)
                    .and_then(|()| card.write_card_memory(r.phys, &buf[..len]).map_err(anyhow::Error::from))
            } else {
                stats.writes += 1;
                stats.bytes_written += len as u64;
                card.read_card_memory(r.phys, &mut buf[..len])
                    .map_err(anyhow::Error::from)
                    .and_then(|()| file.write_all_at(&buf[..len], off).map_err(anyhow::Error::from))
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
pub fn run(card: &Card, ring_base: u64, ring_size: u64, path: &Path) -> Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("open disk image {}", path.display()))?;
    let capacity = file.metadata()?.len() / SECTOR;
    if !wait_for_init(card, Duration::from_secs(120)) {
        eprintln!("[phictl] disk: the card did not reach init; not serving");
        return Ok(());
    }
    let mut mem = ApertureRegion::new(card.aperture(), ring_base as usize, ring_size as usize);
    let (producer, consumer) = loop {
        if let Ok(region) = Region::open(&mem) {
            if let Ok(endpoints) = region.host_endpoints(&mem, ChannelKind::Block) {
                break endpoints;
            }
        }
        thread::sleep(Duration::from_millis(200));
    };
    eprintln!(
        "[phictl] disk: serving {} ({} MiB) as the card's block device",
        path.display(),
        (capacity * SECTOR) >> 20
    );
    let mut buf = vec![0u8; MAX_LEN];
    let mut rec = [0u8; REQ_SIZE];
    let mut stats = Stats::default();
    let mut trace: u32 = if std::env::var_os("PHICTL_DISK_TRACE").is_some() { 40 } else { 0 };
    let mut last_active = Instant::now();
    loop {
        if (consumer.available(&mem) as usize) < REQ_SIZE {
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
        debug_assert_eq!(n, REQ_SIZE);
        let r = Request::parse(&rec);
        if trace > 0 {
            trace -= 1;
            eprintln!(
                "[phictl] disk: rec {n} bytes tag {:#x} op {} len {} sector {} phys {:#x}",
                r.tag, r.op, r.len, r.sector, r.phys
            );
        }
        let status = serve_one(card, &file, capacity, &r, &mut buf, &mut stats);
        while (producer.free(&mem) as usize) < CPL_SIZE {
            thread::sleep(Duration::from_millis(1));
        }
        producer.push(&mut mem, &completion(r.tag, status));
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
