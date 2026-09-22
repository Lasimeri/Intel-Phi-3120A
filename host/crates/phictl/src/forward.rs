//! Root-free TCP forwarding to the card: a userspace network stack
//! (smoltcp) over the ring's Ethernet channel.
//!
//! The `--net` bridge needs a TAP device and therefore root. This module
//! needs neither: phictl itself is the host end of the network, with its
//! own MAC and IP (`10.9.0.1`) on the ring channel, and forwards a host
//! TCP port to a port on the card (`10.9.0.2`): connections accepted on
//! `127.0.0.1:HOSTPORT` become smoltcp TCP connections to the card, bytes
//! are shuttled both ways. `ssh -p 2222 root@localhost` then reaches the
//! card's dropbear from an unprivileged boot. One poll loop at 1 kHz, like
//! the bridge; mutually exclusive with `--net` (both would consume the
//! same ring). See forward.md.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use phi_hw::ringmem::ApertureRegion;
use phi_hw::Card;
use phi_ring::{ChannelKind, Consumer, Producer, Region};
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr};

/// The host end of the ring network as seen by the card. Each card has
/// its own ring, so the same MAC on every one collides with nothing.
const HOST_MAC: [u8; 6] = [0x02, 0x50, 0x48, 0x49, 0x00, 0x01];

/// The host's and the card's addresses from the host's `A.B.C.D/24`: the
/// card is always `.2` of the host's /24 (its init sets that from
/// `PHI_CARD_ADDR`, which `phi-boot.sh` derives from the same index).
pub fn parse_subnet(net_addr: &str) -> Result<(Ipv4Addr, Ipv4Addr)> {
    let ip = net_addr.split('/').next().unwrap_or("");
    let host: Ipv4Addr = ip
        .parse()
        .with_context(|| format!("--net-addr {net_addr:?}: not an IPv4 address"))?;
    let o = host.octets();
    Ok((host, Ipv4Addr::new(o[0], o[1], o[2], 2)))
}
/// Record framing on the network channel (see net.rs).
const MAX_FRAME: usize = 1518;
const HDR: usize = 2;
const ALIGN: usize = 4;
/// Per-connection buffers in the userspace stack.
const SOCKET_BUFFER: usize = 65536;

fn record_len(frame: usize) -> usize {
    (HDR + frame + ALIGN - 1) & !(ALIGN - 1)
}

/// The ring network channel as a smoltcp device: frames in from the
/// card-to-host ring, frames out to the host-to-card ring.
struct RingDevice<'a> {
    mem: ApertureRegion<'a>,
    producer: Producer,
    consumer: Consumer,
    inbound: Vec<u8>,
    frames: VecDeque<Vec<u8>>,
    chunk: Vec<u8>,
}

impl RingDevice<'_> {
    /// Drain the card-to-host ring and cut it into frames.
    fn pump(&mut self) {
        let avail = self.consumer.available(&self.mem) as usize;
        if avail == 0 {
            return;
        }
        let want = avail.min(self.chunk.len());
        let n = self.consumer.pop(&mut self.mem, &mut self.chunk[..want]);
        self.inbound.extend_from_slice(&self.chunk[..n]);
        let mut used = 0;
        while self.inbound.len() - used >= HDR {
            let len = u16::from_le_bytes([self.inbound[used], self.inbound[used + 1]]) as usize;
            let rec = record_len(len);
            if len == 0 || len > MAX_FRAME {
                used = self.inbound.len();
                break;
            }
            if self.inbound.len() - used < rec {
                break;
            }
            self.frames.push_back(self.inbound[used + HDR..used + HDR + len].to_vec());
            used += rec;
        }
        self.inbound.drain(..used);
    }

    /// Put one frame into the host-to-card ring, whole or not at all.
    fn send(&mut self, frame: &[u8]) {
        let rec = record_len(frame.len());
        if frame.len() > MAX_FRAME || (self.producer.free(&self.mem) as usize) < rec {
            return;
        }
        let mut record = Vec::with_capacity(rec);
        record.extend_from_slice(&(frame.len() as u16).to_le_bytes());
        record.extend_from_slice(frame);
        record.resize(rec, 0);
        self.producer.push(&mut self.mem, &record);
    }
}

struct Rx(Vec<u8>);

impl RxToken for Rx {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

struct Tx<'d, 'a>(&'d mut RingDevice<'a>);

impl TxToken for Tx<'_, '_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut frame = vec![0u8; len];
        let r = f(&mut frame);
        self.0.send(&frame);
        r
    }
}

impl<'a> Device for RingDevice<'a> {
    type RxToken<'b>
        = Rx
    where
        Self: 'b;
    type TxToken<'b>
        = Tx<'b, 'a>
    where
        Self: 'b;

    fn receive(&mut self, _now: Instant) -> Option<(Rx, Tx<'_, 'a>)> {
        self.pump();
        let frame = self.frames.pop_front()?;
        Some((Rx(frame), Tx(self)))
    }

    fn transmit(&mut self, _now: Instant) -> Option<Tx<'_, 'a>> {
        Some(Tx(self))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1500;
        caps
    }
}

/// One forwarded connection: the host socket and its smoltcp counterpart.
struct Conn {
    stream: TcpStream,
    handle: SocketHandle,
    host_eof: bool,
    card_eof: bool,
}

/// Parse `HOSTPORT:CARDPORT` (or `HOSTPORT`, card port 22).
pub fn parse_ports(spec: &str) -> Result<(u16, u16)> {
    let (h, c) = match spec.split_once(':') {
        Some((h, c)) => (h, c),
        None => (spec, "22"),
    };
    Ok((
        h.parse().with_context(|| format!("host port in {spec:?}"))?,
        c.parse().with_context(|| format!("card port in {spec:?}"))?,
    ))
}

/// Forward `127.0.0.1:host_port` to the card's `card_port` until the
/// process ends. Waits for the card to reach init like the bridge does.
pub fn run(
    card: &Card,
    ring_base: u64,
    ring_size: u64,
    host_port: u16,
    card_port: u16,
    host_ip: Ipv4Addr,
    card_ip: Ipv4Addr,
) -> Result<()> {
    if !crate::serve::wait_for_init(card, Duration::from_secs(120)) {
        eprintln!("[phictl] forward: the card did not reach init; not forwarding");
        return Ok(());
    }
    let mem = ApertureRegion::new(card.aperture(), ring_base as usize, ring_size as usize);
    let (producer, consumer) = loop {
        if let Ok(region) = Region::open(&mem) {
            if let Ok(endpoints) = region.host_endpoints(&mem, ChannelKind::Network) {
                break endpoints;
            }
        }
        thread::sleep(Duration::from_millis(500));
    };
    let listener = TcpListener::bind(("127.0.0.1", host_port)).with_context(|| format!("listening on 127.0.0.1:{host_port}"))?;
    listener.set_nonblocking(true)?;

    let mut dev = RingDevice {
        mem,
        producer,
        consumer,
        inbound: Vec::new(),
        frames: VecDeque::new(),
        chunk: vec![0u8; 65536],
    };
    let config = Config::new(HardwareAddress::Ethernet(EthernetAddress(HOST_MAC)));
    let mut iface = Interface::new(config, &mut dev, Instant::now());
    iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(IpAddress::Ipv4(host_ip), 24)).expect("one address fits");
    });
    let mut sockets = SocketSet::new(vec![]);
    let mut conns: Vec<Conn> = Vec::new();
    let mut next_port: u16 = 49152;
    let mut buf = vec![0u8; 16384];
    eprintln!("[phictl] forward: 127.0.0.1:{host_port} -> {card_ip}:{card_port} through the ring (no root)");

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(true)?;
                let socket = tcp::Socket::new(
                    tcp::SocketBuffer::new(vec![0; SOCKET_BUFFER]),
                    tcp::SocketBuffer::new(vec![0; SOCKET_BUFFER]),
                );
                let handle = sockets.add(socket);
                let local = next_port;
                next_port = if next_port == u16::MAX { 49152 } else { next_port + 1 };
                let socket = sockets.get_mut::<tcp::Socket>(handle);
                if let Err(e) = socket.connect(iface.context(), (IpAddress::Ipv4(card_ip), card_port), local) {
                    eprintln!("[phictl] forward: connect to the card failed: {e}");
                    sockets.remove(handle);
                    continue;
                }
                conns.push(Conn {
                    stream,
                    handle,
                    host_eof: false,
                    card_eof: false,
                });
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => bail!("accept: {e}"),
        }
        iface.poll(Instant::now(), &mut dev, &mut sockets);

        let mut busy = false;
        conns.retain_mut(|conn| {
            let socket = sockets.get_mut::<tcp::Socket>(conn.handle);
            // Card to host.
            while socket.can_recv() {
                match socket.recv_slice(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        busy = true;
                        if conn.stream.write_all(&buf[..n]).is_err() {
                            socket.abort();
                            break;
                        }
                    }
                }
            }
            // Host to card.
            if !conn.host_eof && socket.can_send() {
                match conn.stream.read(&mut buf) {
                    Ok(0) => {
                        conn.host_eof = true;
                        socket.close();
                    }
                    Ok(n) => {
                        busy = true;
                        let _ = socket.send_slice(&buf[..n]);
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => {
                        conn.host_eof = true;
                        socket.abort();
                    }
                }
            }
            // The card closed its side (FIN received, state CLOSE-WAIT): once its
            // last bytes are out, pass the end of stream on and close ours.
            if socket.state() == tcp::State::CloseWait && !socket.can_recv() && !conn.card_eof {
                conn.card_eof = true;
                let _ = conn.stream.shutdown(Shutdown::Write);
                socket.close();
            }
            if !socket.is_open() && !socket.can_recv() {
                if !conn.card_eof && !conn.host_eof {
                    eprintln!("[phictl] forward: connection to the card ended in state {:?}", socket.state());
                }
                let _ = conn.stream.shutdown(Shutdown::Both);
                sockets.remove(conn.handle);
                return false;
            }
            true
        });
        if !busy {
            thread::sleep(Duration::from_millis(1));
        }
    }
}
