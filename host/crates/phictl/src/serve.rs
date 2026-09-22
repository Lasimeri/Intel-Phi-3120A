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

use std::collections::VecDeque;
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
use phi_rpc::{Decoder, Msg, Traffic};

/// Where card `index`'s control socket lives when nothing says otherwise:
/// the `PHICTL_SOCKET` variable, else the root path when running as root
/// (or, for a client, when a root daemon answers there), else the user's
/// runtime directory (`/tmp/phictl-UID` without one). Card 0 is
/// `phictl/control.sock`, card N `phictl/N/control.sock`
/// (`phi_vfio::cards`). The user path is what an unprivileged `phictl boot
/// --serve` uses: the `phi` group grants the VFIO device, so no root is
/// needed to run the card without a network bridge.
pub fn default_socket(for_bind: bool, index: usize) -> PathBuf {
    if let Ok(p) = std::env::var("PHICTL_SOCKET") {
        return PathBuf::from(p);
    }
    // SAFETY: geteuid has no preconditions.
    let root = unsafe { libc::geteuid() } == 0;
    let root_path = phi_vfio::cards::root_socket_path(index);
    if root || (!for_bind && std::os::unix::net::UnixStream::connect(&root_path).is_ok()) {
        // A root daemon answers there; a stale socket file from an ended one
        // does not, and the user path is tried instead.
        return root_path;
    }
    phi_vfio::cards::socket_path(index)
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

/// Most simultaneous connections to the control socket.
const MAX_CLIENTS: usize = 16;

/// What the card is doing for the session holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Session {
    /// A command; whether the client has yet to close its input.
    Exec { stdin_open: bool },
    /// A file coming in; whether `PutClose` has been sent.
    Put { closed: bool },
    /// A file going out, or a ping: nothing more comes from the client.
    Other,
}

/// One connection to the control socket.
struct Client {
    id: u64,
    stream: UnixStream,
    dec: Decoder,
    /// Session frames decoded while another client holds the card.
    pending: VecDeque<Msg>,
    dead: bool,
}

impl Client {
    fn send(&mut self, m: &Msg) {
        if self.stream.write_all(&m.encode()).is_err() {
            self.dead = true;
        }
    }
}

/// The relay's bookkeeping: who holds the card, who waits for a sample.
#[derive(Default)]
struct Relay {
    clients: Vec<Client>,
    next_id: u64,
    /// The client whose session the card is in, while it is connected.
    holder: Option<u64>,
    /// The session the card is in, until the frame that ends it arrives;
    /// the holder may have left by then.
    session: Option<Session>,
    /// Clients awaiting a `StatReply`, in the order their requests went out.
    stat_queue: VecDeque<u64>,
    /// Bytes for the card not yet in the ring.
    to_card: Vec<u8>,
}

impl Relay {
    /// A frame from client `i`: answered here, queued for the card, or held
    /// until the card is free.
    fn handle(&mut self, i: usize, m: Msg, card: &Card) {
        let id = self.clients[i].id;
        match m {
            Msg::Sensors => self.clients[i].send(&Msg::SensorsReply { text: sensors_text(card) }),
            Msg::Traffic => self.clients[i].send(&Msg::TrafficReply(traffic_now())),
            Msg::Stat => {
                self.to_card.extend_from_slice(&Msg::Stat.encode());
                self.stat_queue.push_back(id);
            }
            m if self.holder == Some(id) => self.forward(m),
            m => self.clients[i].pending.push_back(m),
        }
    }

    /// A session frame to the card, tracking what the session now expects.
    fn forward(&mut self, m: Msg) {
        match &m {
            Msg::Exec { .. } => self.session = Some(Session::Exec { stdin_open: true }),
            Msg::StdinEof => {
                if let Some(Session::Exec { stdin_open }) = &mut self.session {
                    *stdin_open = false;
                }
            }
            Msg::PutOpen { .. } => self.session = Some(Session::Put { closed: false }),
            Msg::PutClose => {
                if let Some(Session::Put { closed }) = &mut self.session {
                    *closed = true;
                }
            }
            Msg::Get { .. } | Msg::Ping => self.session = Some(Session::Other),
            _ => {}
        }
        self.to_card.extend_from_slice(&m.encode());
    }

    /// A frame from the card: a sample to whoever asked first, anything
    /// else to the session holder.
    fn deliver(&mut self, m: Msg) {
        if matches!(m, Msg::StatReply(_)) {
            if let Some(id) = self.stat_queue.pop_front() {
                if let Some(c) = self.clients.iter_mut().find(|c| c.id == id) {
                    c.send(&m);
                }
            }
            return;
        }
        let ends = ends_session(&m);
        let holder = self.holder;
        if let Some(c) = holder.and_then(|id| self.clients.iter_mut().find(|c| c.id == id)) {
            c.send(&m);
        }
        if ends {
            self.session = None;
            self.holder = None;
        }
    }

    /// A departed holder: close what it left open so the card's session ends.
    fn abandon(&mut self) {
        self.holder = None;
        match self.session {
            Some(Session::Exec { stdin_open: true }) => self.forward(Msg::StdinEof),
            Some(Session::Put { closed: false }) => self.forward(Msg::PutClose),
            _ => {}
        }
    }
}

/// The daemon's PCIe byte counters as a message.
fn traffic_now() -> Traffic {
    let [dma_to_card, dma_from_card, aperture_to_card, aperture_from_card] = phi_vfio::traffic::snapshot();
    let [dma_copies_to_card, dma_copies_from_card] = phi_vfio::traffic::copies();
    Traffic {
        dma_to_card,
        dma_from_card,
        aperture_to_card,
        aperture_from_card,
        dma_copies_to_card,
        dma_copies_from_card,
    }
}

/// Relay frames between local clients and the card's rpc channel until the
/// process ends. One client at a time holds the card's session (a command,
/// a transfer, a ping) and the others wait their turn; `Stat` requests from
/// any client are interleaved with the session and their replies routed
/// back in order; `Sensors` and `Traffic` are answered here.
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

    let mut relay = Relay::default();
    let mut from_card = Decoder::new();
    let mut chunk = vec![0u8; 65536];
    loop {
        let mut idle = true;
        // New connections.
        loop {
            match listener.accept() {
                Ok((stream, _)) => match peer_uid(&stream) {
                    Ok(uid) if uid == 0 || uid == owner => {
                        if relay.clients.len() >= MAX_CLIENTS {
                            eprintln!("[phictl] serve: refused a connection: {MAX_CLIENTS} clients already");
                            continue;
                        }
                        stream.set_nonblocking(true)?;
                        relay.next_id += 1;
                        relay.clients.push(Client {
                            id: relay.next_id,
                            stream,
                            dec: Decoder::new(),
                            pending: VecDeque::new(),
                            dead: false,
                        });
                    }
                    Ok(uid) => eprintln!("[phictl] serve: refused a connection from uid {uid}"),
                    Err(e) => eprintln!("[phictl] serve: {e:#}"),
                },
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e).context("accept"),
            }
        }
        // Clients to card. A client waiting for the card keeps its later
        // frames in its socket until its turn.
        for i in 0..relay.clients.len() {
            let c = &relay.clients[i];
            if c.dead || (!c.pending.is_empty() && relay.holder != Some(c.id)) {
                continue;
            }
            match relay.clients[i].stream.read(&mut chunk) {
                Ok(0) => relay.clients[i].dead = true,
                Ok(n) => {
                    idle = false;
                    relay.clients[i].dec.push(&chunk[..n]);
                    loop {
                        match relay.clients[i].dec.next_frame() {
                            Ok(Some(m)) => relay.handle(i, m, card),
                            Ok(None) => break,
                            Err(e) => {
                                eprintln!("[phictl] serve: bad frame from a client: {e}");
                                relay.clients[i].dead = true;
                                break;
                            }
                        }
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                Err(_) => relay.clients[i].dead = true,
            }
        }
        if relay.clients.iter().any(|c| c.dead && relay.holder == Some(c.id)) {
            relay.abandon();
        }
        relay.clients.retain(|c| !c.dead);
        // The card is free: the next client waiting for it.
        if relay.session.is_none() {
            if let Some(i) = relay.clients.iter().position(|c| !c.pending.is_empty()) {
                relay.holder = Some(relay.clients[i].id);
                let pending: Vec<Msg> = relay.clients[i].pending.drain(..).collect();
                for m in pending {
                    relay.forward(m);
                }
                idle = false;
            }
        }
        if !relay.to_card.is_empty() {
            let free = producer.free(&mem) as usize;
            let n = relay.to_card.len().min(free);
            if n > 0 {
                let written = producer.push(&mut mem, &relay.to_card[..n]);
                relay.to_card.drain(..written);
                idle = false;
            }
        }
        // Card to clients.
        let avail = consumer.available(&mem) as usize;
        if avail > 0 {
            idle = false;
            let want = avail.min(chunk.len());
            let n = consumer.pop(&mut mem, &mut chunk[..want]);
            from_card.push(&chunk[..n]);
            loop {
                match from_card.next_frame() {
                    Ok(Some(m)) => relay.deliver(m),
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("[phictl] serve: bad frame from the card: {e}; discarding pending bytes");
                        from_card = Decoder::new();
                        break;
                    }
                }
            }
        }
        if idle {
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
