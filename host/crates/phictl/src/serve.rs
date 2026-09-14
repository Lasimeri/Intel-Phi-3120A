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
use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use phi_hw::ringmem::ApertureRegion;
use phi_hw::Card;
use phi_ring::{ChannelKind, Region};
use phi_rpc::{Decoder, Msg};

/// Default control socket path.
pub const DEFAULT_SOCKET: &str = "/run/phictl/control.sock";

/// The uid that may use the socket when none is given: the user behind
/// `sudo`, or root.
pub fn default_owner() -> u32 {
    std::env::var("SUDO_UID").ok().and_then(|s| s.parse().ok()).unwrap_or(0)
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
pub fn run(card: &Card, ring_base: u64, ring_size: u64, listener: UnixListener, owner: u32) -> Result<()> {
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
