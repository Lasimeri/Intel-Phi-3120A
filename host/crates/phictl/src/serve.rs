//! `phictl boot --serve`: a local control socket through which the card is
//! driven without SSH.
//!
//! The process that booted the card holds the VFIO group and is the only
//! one that can reach the ring region, so it is also the one that relays
//! rpc frames (`phi-rpc`) between a local client and the card's agent. The
//! socket is the whole attack surface, and it is kept small: it lives in a
//! root-owned directory that is traversable but not listable (0711), the
//! socket file belongs to one owner uid with mode 0600, and every accepted
//! connection is checked against that uid (or root) with `SO_PEERCRED`
//! before a byte is relayed. Nothing the card sends is interpreted by the
//! daemon beyond frame boundaries; nothing is executed on the host. One
//! client at a time; a client that disconnects mid-command leaves the
//! daemon draining the card's replies until that session ends, so the next
//! client starts clean. See serve.md.

use std::ffi::CString;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use phi_hw::ringmem::ApertureRegion;
use phi_hw::Card;
use phi_regs::sbox;
use phi_ring::{ChannelKind, Region};
use phi_rpc::{Decoder, Msg};

/// Control socket path when the daemon runs as root.
pub const ROOT_SOCKET: &str = "/run/phictl/control.sock";

/// Where the control socket lives when nothing says otherwise: the
/// `PHICTL_SOCKET` variable, else the root path when running as root (or, for
/// a client, when a root daemon has one), else `/phictl/control.sock`
/// (`/tmp/phictl-UID` without a runtime directory). The user path is what an
/// unprivileged `phictl boot --serve` uses: the `phi` group grants the VFIO
/// device, so no root is needed to run the card without a network bridge.
pub fn default_socket(for_bind: bool) -> PathBuf {
    if let Ok(p) = std::env::var("PHICTL_SOCKET") {
        return PathBuf::from(p);
    }
    // SAFETY: geteuid has no preconditions.
    let root = unsafe { libc::geteuid() } == 0;
    if root || (!for_bind && std::os::unix::net::UnixStream::connect(ROOT_SOCKET).is_ok()) {
        // A root daemon answers there; a stale socket file from an ended one
        // does not, and the user path is tried instead.
        return PathBuf::from(ROOT_SOCKET);
    }
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        // SAFETY: getuid has no preconditions.
        .unwrap_or_else(|_| PathBuf::from(format!("/tmp/phictl-{}", unsafe { libc::getuid() })));
    dir.join("phictl").join("control.sock")
}

/// The uid that may use the socket when none is given: the user behind
/// `sudo`, else the daemon's own uid.
pub fn default_owner() -> u32 {
    std::env::var("SUDO_UID")
        .ok()
        .and_then(|s| s.parse().ok())
        // SAFETY: geteuid has no preconditions.
        .unwrap_or_else(|| unsafe { libc::geteuid() })
}

/// Uid of the process at the other end of a Unix socket.
fn peer_uid(stream: &UnixStream) -> Result<u32> {
    // SAFETY: an all-zero ucred is a valid out-parameter; the kernel fills it.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the pointers refer to the locals above for the duration of the call.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("SO_PEERCRED");
    }
    Ok(cred.uid)
}

/// Create the socket: parent directory root 0711, socket file owned by
/// `owner` with mode 0600, stale socket removed first.
pub fn bind(path: &Path, owner: u32) -> Result<UnixListener> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o711))?;
    }
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let c = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: a NUL-terminated path; gid -1 leaves the group unchanged.
    if unsafe { libc::chown(c.as_ptr(), owner, u32::MAX) } != 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| format!("chown {} to uid {owner}", path.display()));
    }
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Does this frame end the session the card is answering?
fn ends_session(m: &Msg) -> bool {
    matches!(m, Msg::Exit(_) | Msg::Error(_) | Msg::GetEnd { .. } | Msg::Pong { .. })
}

/// Relay frames between one local client at a time and the card's rpc
/// channel until the process ends.
/// The card's sensors as text, read from the SBOX through the MMIO BAR:
/// die and board temperatures, core voltage and clock (decoding in
/// `phi_regs::sbox::sensors`, from Intel's RAS module).
pub fn sensors_text(card: &Card) -> String {
    use sbox::sensors::*;
    let die = die_temps([
        card.sbox_read(sbox::CURRENT_DIE_TEMP0),
        card.sbox_read(sbox::CURRENT_DIE_TEMP0 + 4),
        card.sbox_read(sbox::CURRENT_DIE_TEMP0 + 8),
    ]);
    let max = die_temps([
        card.sbox_read(sbox::MAX_DIE_TEMP0),
        card.sbox_read(sbox::MAX_DIE_TEMP0 + 4),
        card.sbox_read(sbox::MAX_DIE_TEMP0 + 8),
    ]);
    let (inlet, vccp) = board_temps(card.sbox_read(sbox::BOARD_TEMP1));
    let (gddr, gddr_vr) = board_temps(card.sbox_read(sbox::BOARD_TEMP2));
    let vddg = vddg_temp(card.sbox_read(sbox::STATUS_FAN2));
    let tmu = tmu_temp(card.sbox_read(sbox::THERMAL_STATUS));
    let corevolt = card.sbox_read(sbox::COREVOLT);
    let corefreq = card.sbox_read(sbox::COREFREQ);
    let ratio = card.sbox_read(sbox::CURRENT_CLK_RATIO);
    let scratch4 = card.sbox_read(sbox::spad(4));
    let opt = |v: Option<u16>| v.map(|t| format!("{t} C")).unwrap_or_else(|| "n/a".into());
    let khz = |r: u32| {
        core_khz(r & 0xfff, scratch4)
            .map(|k| format!("{} MHz", k / 1000))
            .unwrap_or_else(|| format!("n/a (code {:#x})", r & 0xfff))
    };
    let mut s = String::new();
    s.push_str(&format!(
        "die temperatures: {} C (max seen {} C)\n",
        die.iter()
            .map(|t| if *t == 0 { "n/a".to_string() } else { t.to_string() })
            .collect::<Vec<_>>()
            .join(" "),
        max.iter().max().copied().unwrap_or(0)
    ));
    s.push_str(&format!(
        "board: inlet {}, vccp regulator {}, gddr {}, gddr regulator {}, vddg regulator {} C, tmu die {}\n",
        opt(inlet),
        opt(vccp),
        opt(gddr),
        opt(gddr_vr),
        vddg,
        opt(tmu)
    ));
    s.push_str(&format!(
        "core voltage: {}\n",
        vcore_mv(corevolt).map(|mv| format!("{mv} mV")).unwrap_or_else(|| "n/a".into())
    ));
    s.push_str(&format!(
        "core clock: {} (COREFREQ), {} (CURRENT_CLK_RATIO)\n",
        khz(corefreq),
        khz(ratio)
    ));
    s
}

pub fn run(card: &Card, ring_base: u64, ring_size: u64, listener: UnixListener, owner: u32) -> Result<()> {
    if !wait_for_init(card, Duration::from_secs(120)) {
        eprintln!("[phictl] serve: the card did not reach init; not relaying");
        return Ok(());
    }
    let mut mem = ApertureRegion::new(card.aperture(), ring_base as usize, ring_size as usize);
    let (producer, consumer) = loop {
        if let Ok(region) = Region::open(&mem) {
            if let Ok(endpoints) = region.host_endpoints(&mem, ChannelKind::Rpc) {
                break endpoints;
            }
        }
        thread::sleep(Duration::from_millis(500));
    };
    eprintln!("[phictl] serve: control socket ready for uid {owner} (and root)");

    let mut client: Option<UnixStream> = None;
    let mut from_client = Decoder::new();
    let mut from_card = Decoder::new();
    let mut in_exec = false;
    let mut to_card: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 65536];
    loop {
        let mut idle = true;
        if client.is_none() {
            match listener.accept() {
                Ok((stream, _)) => match peer_uid(&stream) {
                    Ok(uid) if uid == 0 || uid == owner => {
                        stream.set_read_timeout(Some(Duration::from_millis(1)))?;
                        client = Some(stream);
                        from_client = Decoder::new();
                    }
                    Ok(uid) => eprintln!("[phictl] serve: refused a connection from uid {uid}"),
                    Err(e) => eprintln!("[phictl] serve: {e:#}"),
                },
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => return Err(e).context("accept"),
            }
        }
        // Client to card.
        if let Some(stream) = client.as_mut() {
            match stream.read(&mut chunk) {
                Ok(0) => {
                    client = None;
                    if in_exec {
                        to_card.extend_from_slice(&Msg::StdinEof.encode());
                    }
                }
                Ok(n) => {
                    idle = false;
                    from_client.push(&chunk[..n]);
                    loop {
                        match from_client.next_frame() {
                            Ok(Some(Msg::Sensors)) => {
                                // Answered here; the card never sees it.
                                let reply = Msg::SensorsReply { text: sensors_text(card) };
                                if let Some(stream) = client.as_mut() {
                                    if stream.write_all(&reply.encode()).is_err() {
                                        client = None;
                                    }
                                }
                                break;
                            }
                            Ok(Some(m)) => {
                                if matches!(m, Msg::Exec { .. }) {
                                    in_exec = true;
                                }
                                to_card.extend_from_slice(&m.encode());
                            }
                            Ok(None) => break,
                            Err(e) => {
                                eprintln!("[phictl] serve: bad frame from the client: {e}");
                                client = None;
                                break;
                            }
                        }
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => {}
                Err(_) => client = None,
            }
        }
        if !to_card.is_empty() {
            let free = producer.free(&mem) as usize;
            let n = to_card.len().min(free);
            if n > 0 {
                let written = producer.push(&mut mem, &to_card[..n]);
                to_card.drain(..written);
                idle = false;
            }
        }
        // Card to client.
        let avail = consumer.available(&mem) as usize;
        if avail > 0 {
            idle = false;
            let want = avail.min(chunk.len());
            let n = consumer.pop(&mut mem, &mut chunk[..want]);
            from_card.push(&chunk[..n]);
            loop {
                match from_card.next_frame() {
                    Ok(Some(m)) => {
                        if ends_session(&m) {
                            in_exec = false;
                        }
                        if let Some(stream) = client.as_mut() {
                            if stream.write_all(&m.encode()).is_err() {
                                client = None;
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("[phictl] serve: bad frame from the card: {e}; discarding pending bytes");
                        from_card = Decoder::new();
                        break;
                    }
                }
            }
        }
        if idle && client.is_none() {
            thread::sleep(Duration::from_millis(1));
        }
    }
}

/// Block until the card's kernel reports that init has started (POST "K7"),
/// or until `limit` passes: neither the bridge nor the relay may read or
/// write card memory during early boot, when the kernel is still setting up
/// its memory map, caches and MTRRs.
pub fn wait_for_init(card: &Card, limit: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < limit {
        let text = card.postcode().text();
        if text == "K7" || text == "KH" || text == "KP" {
            return text == "K7";
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}
