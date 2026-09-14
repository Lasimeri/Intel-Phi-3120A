//! TAP bridge between the host and the card's ring network channel.
//!
//! The ring region's kind-2 channel carries Ethernet frames as records of
//! `u16` length, payload, padding to 4 bytes (`docs/spec/ring-protocol.md`).
//! This module creates a TAP device, assigns it an address, and moves frames
//! between the device and the two rings from one thread: frames read from
//! the TAP go into the host-to-card ring whole (dropped when it is full,
//! like a NIC queue), bytes drained from the card-to-host ring are
//! reassembled into records and written to the TAP. Both directions are
//! polled at 1 kHz; the card driver (`arch/x86/kernel/knc_net.c` in the
//! kernel series) polls at the same rate. See net.md.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use phi_hw::ringmem::ApertureRegion;
use phi_hw::Card;
use phi_ring::{ChannelKind, Region};

/// Ethernet frame with a VLAN tag and no FCS: the largest record either
/// side accepts (the card driver's KNC_NET_MAX_FRAME).
const MAX_FRAME: usize = 1518;
/// Record header: the frame length as little-endian u16.
const HDR: usize = 2;
/// Records are padded to this alignment.
const ALIGN: usize = 4;

fn record_len(frame: usize) -> usize {
    (HDR + frame + ALIGN - 1) & !(ALIGN - 1)
}

/// Create (or attach to) the TAP device `name`, opened for raw frames
/// without the packet-information header.
fn open_tap(name: &str) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/net/tun")
        .context("opening /dev/net/tun")?;
    // SAFETY: an all-zero ifreq is a valid value; the fields set below are
    // the ones TUNSETIFF reads.
    let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
    if name.len() >= ifr.ifr_name.len() {
        bail!("TAP name {name:?} is too long");
    }
    for (dst, src) in ifr.ifr_name.iter_mut().zip(name.bytes()) {
        *dst = src as libc::c_char;
    }
    ifr.ifr_ifru.ifru_flags = (libc::IFF_TAP | libc::IFF_NO_PI) as libc::c_short;
    // SAFETY: TUNSETIFF reads one ifreq through the pointer; it outlives the call.
    if unsafe { libc::ioctl(file.as_raw_fd(), libc::TUNSETIFF, &ifr as *const libc::ifreq) } < 0 {
        return Err(std::io::Error::last_os_error()).context("TUNSETIFF on /dev/net/tun (needs root)");
    }
    Ok(file)
}

/// Give the TAP device an address and bring it up with iproute2 (`ip addr
/// replace` is idempotent, so a reused device is fine).
fn configure(name: &str, addr: &str) -> Result<()> {
    for args in [vec!["addr", "replace", addr, "dev", name], vec!["link", "set", name, "up"]] {
        let status = Command::new("ip")
            .args(&args)
            .status()
            .with_context(|| format!("running ip {}", args.join(" ")))?;
        if !status.success() {
            bail!("ip {} failed: {status}", args.join(" "));
        }
    }
    Ok(())
}

/// Wait up to `timeout` for a frame to be readable from the TAP.
fn readable(file: &File, timeout: Duration) -> bool {
    let mut pfd = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one pollfd, valid for the duration of the call.
    unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as libc::c_int) > 0 && pfd.revents & libc::POLLIN != 0 }
}

/// Bridge frames between the TAP device `name` (assigned `addr`) and the
/// card's network channel until the process ends. The ring region is
/// written before the kernel starts, but this thread may run first, so it
/// waits for a valid header.
pub fn bridge(card: &Card, ring_base: u64, ring_size: u64, name: &str, addr: &str) -> Result<()> {
    let mut tap = open_tap(name)?;
    configure(name, addr)?;
    let mut mem = ApertureRegion::new(card.aperture(), ring_base as usize, ring_size as usize);
    let (producer, consumer) = loop {
        if let Ok(region) = Region::open(&mem) {
            if let Ok(endpoints) = region.host_endpoints(&mem, ChannelKind::Network) {
                break endpoints;
            }
        }
        thread::sleep(Duration::from_millis(500));
    };
    eprintln!("[phictl] net: {name} ({addr}) bridged to the ring network channel");

    let mut frame = vec![0u8; MAX_FRAME + 64];
    let mut record = Vec::with_capacity(record_len(MAX_FRAME));
    let mut inbound: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 65536];
    let (mut to_card, mut from_card, mut dropped, mut bad) = (0u64, 0u64, 0u64, 0u64);
    let mut reported = (0u64, 0u64, 0u64, 0u64);
    let mut next_report = Instant::now() + Duration::from_secs(30);
    loop {
        // Host to card: one frame per poll round, whole or not at all.
        if readable(&tap, Duration::from_millis(1)) {
            let n = tap.read(&mut frame).unwrap_or(0);
            if n > 0 && n <= MAX_FRAME {
                let rec = record_len(n);
                if producer.free(&mem) as usize >= rec {
                    record.clear();
                    record.extend_from_slice(&(n as u16).to_le_bytes());
                    record.extend_from_slice(&frame[..n]);
                    record.resize(rec, 0);
                    producer.push(&mut mem, &record);
                    to_card += 1;
                } else {
                    dropped += 1;
                }
            }
        }
        // Card to host: drain what the card published, reassemble records.
        let avail = consumer.available(&mem) as usize;
        if avail > 0 {
            let want = avail.min(chunk.len());
            let n = consumer.pop(&mut mem, &mut chunk[..want]);
            inbound.extend_from_slice(&chunk[..n]);
            let mut used = 0;
            while inbound.len() - used >= HDR {
                let len = u16::from_le_bytes([inbound[used], inbound[used + 1]]) as usize;
                let rec = record_len(len);
                if len == 0 || len > MAX_FRAME {
                    // Not a record the card would have published: resync.
                    bad += 1;
                    used = inbound.len();
                    break;
                }
                if inbound.len() - used < rec {
                    break;
                }
                if tap.write_all(&inbound[used + HDR..used + HDR + len]).is_ok() {
                    from_card += 1;
                }
                used += rec;
            }
            inbound.drain(..used);
        }
        if Instant::now() >= next_report {
            let now = (to_card, from_card, dropped, bad);
            if now != reported {
                eprintln!(
                    "[phictl] net: {to_card} frames to the card, {from_card} from it, {dropped} dropped (ring full), {bad} bad records"
                );
                reported = now;
            }
            next_report = Instant::now() + Duration::from_secs(30);
        }
    }
}
