//! Message framing for the ring's rpc channel (kind 3).
//!
//! The host daemon (`phictl serve`) and the card agent (`card/agent`)
//! exchange these messages as a byte stream in each direction. One
//! session runs at a time: a command with its input and output, or a file
//! transfer. The encoding is deliberately plain so that both ends, built by
//! different toolchains, agree byte for byte:
//!
//! ```text
//! frame   := u32 length (of what follows, little-endian) | u8 tag | body
//! string  := u16 length | bytes (UTF-8, at most 65535 bytes)
//! bytes   := the rest of the body
//! list    := u16 count | items
//! ```
//!
//! Limits, and what the encoder does at them (the wire cannot say "too
//! big", so the encoder clips and the receiver never sees an invalid
//! frame): a string longer than [`MAX_STRING`] bytes is cut at the last
//! character boundary within the limit, a list longer than [`MAX_LIST`]
//! keeps its first `MAX_LIST` items, and a byte payload (`Stdin`,
//! `Stdout`, `PutData`, ...) is the sender's to keep under [`MAX_FRAME`]:
//! the receiver rejects a longer frame and loses its place in the stream.
//! The agent and the client send data frames of at most 64 KiB.
//!
//! Nothing here touches the ring itself; `phi-ring` and `/dev/phirpc` carry
//! the frames. See lib.md.

use std::fmt;

/// Largest frame either side accepts: the `length` field's maximum, which
/// counts the tag and the body (so the whole frame is four bytes more).
pub const MAX_FRAME: usize = 1 << 20;
/// Longest string, in bytes (a `u16` length prefix).
pub const MAX_STRING: usize = u16::MAX as usize;
/// Longest list (a `u16` count).
pub const MAX_LIST: usize = u16::MAX as usize;

/// A message on the rpc channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Host to card: start a command. `argv[0]` is the program. An absent
    /// `cwd` is encoded as an empty string, so `Some("")` decodes as `None`.
    Exec {
        argv: Vec<String>,
        env: Vec<(String, String)>,
        cwd: Option<String>,
    },
    /// Host to card: bytes for the command's standard input.
    Stdin(Vec<u8>),
    /// Host to card: standard input is closed.
    StdinEof,
    /// Card to host: bytes the command wrote to standard output.
    Stdout(Vec<u8>),
    /// Card to host: bytes the command wrote to standard error.
    Stderr(Vec<u8>),
    /// Card to host: the command ended (exit status, or 128 + signal).
    Exit(i32),
    /// Host to card: create or truncate a file, then expect `PutData`/`PutClose`.
    PutOpen { path: String, mode: u32 },
    /// Host to card: file contents, in order.
    PutData(Vec<u8>),
    /// Host to card: the file is complete.
    PutClose,
    /// Host to card: send this file back as `GetData` frames, then `GetEnd`.
    Get { path: String },
    /// Card to host: file contents, in order.
    GetData(Vec<u8>),
    /// Card to host: the file is complete; `size` bytes were sent.
    GetEnd { size: u64 },
    /// Either direction: the request failed; the session is over.
    Error(String),
    /// Host to card: liveness check.
    Ping,
    /// Card to host: reply to `Ping`, with the agent's version.
    Pong { version: String },
    /// Client to the host daemon only: read the card's sensors from the SBOX
    /// (never relayed to the card; the daemon answers with `SensorsReply`).
    Sensors,
    /// Host daemon to client: the sensors as text, one reading per line.
    SensorsReply { text: String },
    /// Host to card: sample the card's load, memory, sensors and processes.
    /// The agent answers at once, also while a command or a transfer runs,
    /// so the daemon interleaves it with a session and routes the reply
    /// back to whoever asked.
    Stat,
    /// Card to host: one sample.
    StatReply(Box<Stat>),
    /// Client to the host daemon only: the bytes the daemon has moved over
    /// PCIe since it started; answered with `TrafficReply`.
    Traffic,
    /// Host daemon to client: cumulative byte counts by path and direction.
    TrafficReply(Traffic),
}

/// Why a frame could not be decoded.
#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Frame length exceeds [`MAX_FRAME`] or is zero.
    Length(u32),
    /// Unknown message tag.
    Tag(u8),
    /// Body shorter than its fields require, or not UTF-8 where a string is.
    Malformed,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Length(n) => write!(f, "frame length {n} out of range"),
            DecodeError::Tag(t) => write!(f, "unknown message tag {t:#x}"),
            DecodeError::Malformed => write!(f, "malformed frame body"),
        }
    }
}

impl std::error::Error for DecodeError {}

const TAG_EXEC: u8 = 1;
const TAG_STDIN: u8 = 2;
const TAG_STDIN_EOF: u8 = 3;
const TAG_STDOUT: u8 = 4;
const TAG_STDERR: u8 = 5;
const TAG_EXIT: u8 = 6;
const TAG_PUT_OPEN: u8 = 7;
const TAG_PUT_DATA: u8 = 8;
const TAG_PUT_CLOSE: u8 = 9;
const TAG_GET: u8 = 10;
const TAG_GET_DATA: u8 = 11;
const TAG_GET_END: u8 = 12;
const TAG_ERROR: u8 = 13;
const TAG_PING: u8 = 14;
const TAG_PONG: u8 = 15;
const TAG_SENSORS: u8 = 16;
const TAG_SENSORS_REPLY: u8 = 17;
const TAG_STAT: u8 = 18;
const TAG_STAT_REPLY: u8 = 19;
const TAG_TRAFFIC: u8 = 20;
const TAG_TRAFFIC_REPLY: u8 = 21;

/// One sample of the card's state, as the agent reads it from /proc and
/// sysfs. Counters are cumulative; the receiver differences two samples.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    /// Milliseconds since the card's kernel booted (/proc/uptime).
    pub uptime_ms: u64,
    /// One entry per CPU, in /proc/stat order.
    pub cpus: Vec<CpuStat>,
    /// /proc/meminfo, in kB.
    pub mem: MemStat,
    /// Load averages over 1, 5 and 15 minutes, in hundredths; the wire
    /// type caps each at 655.35 (a runaway on 228 threads can exceed it).
    pub load: [u16; 3],
    /// Runnable tasks and all tasks (the fourth field of /proc/loadavg).
    pub running: u32,
    pub tasks: u32,
    /// hwmon temperatures temp1.. in degrees C, -1 where a sensor is absent.
    pub temps: Vec<i16>,
    /// Highest die temperature recorded since boot (temp*_max), -1 if unknown.
    pub temp_peak: i16,
    /// Core voltage in mV and clock in MHz (hwmon in0_input, core_mhz).
    pub vcore_mv: u32,
    pub core_mhz: u32,
    /// Block devices: bytes read and written (/proc/diskstats sectors times 512).
    pub disks: Vec<DevStat>,
    /// Network interfaces: bytes received and sent (/proc/net/dev).
    pub nets: Vec<DevStat>,
    /// Every user process, and each kernel thread whose CPU time moved
    /// since the agent's previous sample.
    pub procs: Vec<ProcStat>,
    /// All processes in /proc, and how many of them are kernel threads.
    pub nprocs: u32,
    pub nkthreads: u32,
}

/// One CPU's counters from /proc/stat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuStat {
    /// Physical core (sysfs topology/core_id).
    pub core: u16,
    /// Ticks (USER_HZ, 100 per second) spent busy: user, nice, system, irq,
    /// softirq and steal. The kernel's counters are 64-bit; these are their
    /// low 32 bits, which wrap after 497 days of one state.
    pub busy: u32,
    /// Ticks spent idle or waiting for I/O.
    pub idle: u32,
}

/// /proc/meminfo, in kB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemStat {
    pub total_kb: u64,
    pub avail_kb: u64,
    pub buffers_kb: u64,
    pub cached_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
}

/// A device's two byte counters: read and written, or received and sent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DevStat {
    pub name: String,
    pub read: u64,
    pub written: u64,
}

/// One process from /proc/[pid]/stat.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcStat {
    pub pid: u32,
    /// The state letter (R, S, D, Z, T and so on).
    pub state: u8,
    /// PF_KTHREAD in the flags field.
    pub kthread: bool,
    /// utime plus stime, in ticks.
    pub ticks: u64,
    /// Resident set, in kB.
    pub rss_kb: u64,
    pub threads: u32,
    pub comm: String,
}

/// Bytes the host daemon has moved over PCIe, cumulative since it started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Traffic {
    pub dma_to_card: u64,
    pub dma_from_card: u64,
    pub aperture_to_card: u64,
    pub aperture_from_card: u64,
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// A list count, clipped to [`MAX_LIST`]; returns the count written, which
/// is how many items the caller must then write.
fn put_count(out: &mut Vec<u8>, n: usize) -> usize {
    let n = n.min(MAX_LIST);
    put_u16(out, n as u16);
    n
}

/// The longest prefix of `s` that is at most `max` bytes and ends on a
/// character boundary, so that a clipped string is still UTF-8.
fn clip(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// A string, clipped to [`MAX_STRING`] bytes at a character boundary.
fn put_str(out: &mut Vec<u8>, s: &str) {
    let b = clip(s, MAX_STRING).as_bytes();
    put_u16(out, b.len() as u16);
    out.extend_from_slice(b);
}

fn put_devs(out: &mut Vec<u8>, list: &[DevStat]) {
    let n = put_count(out, list.len());
    for d in &list[..n] {
        put_str(out, &d.name);
        put_u64(out, d.read);
        put_u64(out, d.written);
    }
}

fn put_stat(out: &mut Vec<u8>, s: &Stat) {
    put_u64(out, s.uptime_ms);
    let n = put_count(out, s.cpus.len());
    for c in &s.cpus[..n] {
        put_u16(out, c.core);
        put_u32(out, c.busy);
        put_u32(out, c.idle);
    }
    for v in [
        s.mem.total_kb,
        s.mem.avail_kb,
        s.mem.buffers_kb,
        s.mem.cached_kb,
        s.mem.swap_total_kb,
        s.mem.swap_free_kb,
    ] {
        put_u64(out, v);
    }
    for l in s.load {
        put_u16(out, l);
    }
    put_u32(out, s.running);
    put_u32(out, s.tasks);
    let n = put_count(out, s.temps.len());
    for t in &s.temps[..n] {
        put_u16(out, *t as u16);
    }
    put_u16(out, s.temp_peak as u16);
    put_u32(out, s.vcore_mv);
    put_u32(out, s.core_mhz);
    put_devs(out, &s.disks);
    put_devs(out, &s.nets);
    let n = put_count(out, s.procs.len());
    for p in &s.procs[..n] {
        put_u32(out, p.pid);
        out.push(p.state);
        out.push(p.kthread as u8);
        put_u64(out, p.ticks);
        put_u64(out, p.rss_kb);
        put_u32(out, p.threads);
        put_str(out, &p.comm);
    }
    put_u32(out, s.nprocs);
    put_u32(out, s.nkthreads);
}

/// A cursor over one frame body. Fields are read in wire order; running
/// out of bytes is `Malformed`. Bytes after the last field are ignored.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if n > self.buf.len() - self.pos {
            return Err(DecodeError::Malformed);
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().map_err(|_| DecodeError::Malformed)?))
    }
    fn str(&mut self) -> Result<String, DecodeError> {
        let n = self.u16()? as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| DecodeError::Malformed)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16, DecodeError> {
        Ok(self.u16()? as i16)
    }
    fn devs(&mut self) -> Result<Vec<DevStat>, DecodeError> {
        let n = self.u16()? as usize;
        let mut v = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            v.push(DevStat {
                name: self.str()?,
                read: self.u64()?,
                written: self.u64()?,
            });
        }
        Ok(v)
    }
    fn stat(&mut self) -> Result<Stat, DecodeError> {
        let mut s = Stat {
            uptime_ms: self.u64()?,
            ..Stat::default()
        };
        let n = self.u16()? as usize;
        s.cpus.reserve(n.min(1024));
        for _ in 0..n {
            s.cpus.push(CpuStat {
                core: self.u16()?,
                busy: self.u32()?,
                idle: self.u32()?,
            });
        }
        s.mem = MemStat {
            total_kb: self.u64()?,
            avail_kb: self.u64()?,
            buffers_kb: self.u64()?,
            cached_kb: self.u64()?,
            swap_total_kb: self.u64()?,
            swap_free_kb: self.u64()?,
        };
        s.load = [self.u16()?, self.u16()?, self.u16()?];
        s.running = self.u32()?;
        s.tasks = self.u32()?;
        let n = self.u16()? as usize;
        s.temps.reserve(n.min(64));
        for _ in 0..n {
            s.temps.push(self.i16()?);
        }
        s.temp_peak = self.i16()?;
        s.vcore_mv = self.u32()?;
        s.core_mhz = self.u32()?;
        s.disks = self.devs()?;
        s.nets = self.devs()?;
        let n = self.u16()? as usize;
        s.procs.reserve(n.min(4096));
        for _ in 0..n {
            s.procs.push(ProcStat {
                pid: self.u32()?,
                state: self.u8()?,
                kthread: self.u8()? != 0,
                ticks: self.u64()?,
                rss_kb: self.u64()?,
                threads: self.u32()?,
                comm: self.str()?,
            });
        }
        s.nprocs = self.u32()?;
        s.nkthreads = self.u32()?;
        Ok(s)
    }
    fn rest(&mut self) -> Vec<u8> {
        let s = self.buf[self.pos..].to_vec();
        self.pos = self.buf.len();
        s
    }
}

impl Msg {
    /// The message's name, for logs: its payload (which may be a data
    /// chunk of 64 KiB) is left out.
    pub fn name(&self) -> &'static str {
        match self {
            Msg::Exec { .. } => "Exec",
            Msg::Stdin(_) => "Stdin",
            Msg::StdinEof => "StdinEof",
            Msg::Stdout(_) => "Stdout",
            Msg::Stderr(_) => "Stderr",
            Msg::Exit(_) => "Exit",
            Msg::PutOpen { .. } => "PutOpen",
            Msg::PutData(_) => "PutData",
            Msg::PutClose => "PutClose",
            Msg::Get { .. } => "Get",
            Msg::GetData(_) => "GetData",
            Msg::GetEnd { .. } => "GetEnd",
            Msg::Error(_) => "Error",
            Msg::Ping => "Ping",
            Msg::Pong { .. } => "Pong",
            Msg::Sensors => "Sensors",
            Msg::SensorsReply { .. } => "SensorsReply",
            Msg::Stat => "Stat",
            Msg::StatReply(_) => "StatReply",
            Msg::Traffic => "Traffic",
            Msg::TrafficReply(_) => "TrafficReply",
        }
    }

    /// Serialise as one frame (length prefix included). Strings and lists
    /// are clipped to their wire limits (see the crate documentation); a
    /// byte payload is the caller's to keep under [`MAX_FRAME`], and a
    /// frame over it is asserted against in debug builds.
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        let tag = match self {
            Msg::Exec { argv, env, cwd } => {
                let n = put_count(&mut body, argv.len());
                for a in &argv[..n] {
                    put_str(&mut body, a);
                }
                let n = put_count(&mut body, env.len());
                for (k, v) in &env[..n] {
                    put_str(&mut body, k);
                    put_str(&mut body, v);
                }
                put_str(&mut body, cwd.as_deref().unwrap_or(""));
                TAG_EXEC
            }
            Msg::Stdin(d) => {
                body.extend_from_slice(d);
                TAG_STDIN
            }
            Msg::StdinEof => TAG_STDIN_EOF,
            Msg::Stdout(d) => {
                body.extend_from_slice(d);
                TAG_STDOUT
            }
            Msg::Stderr(d) => {
                body.extend_from_slice(d);
                TAG_STDERR
            }
            Msg::Exit(code) => {
                body.extend_from_slice(&code.to_le_bytes());
                TAG_EXIT
            }
            Msg::PutOpen { path, mode } => {
                put_str(&mut body, path);
                body.extend_from_slice(&mode.to_le_bytes());
                TAG_PUT_OPEN
            }
            Msg::PutData(d) => {
                body.extend_from_slice(d);
                TAG_PUT_DATA
            }
            Msg::PutClose => TAG_PUT_CLOSE,
            Msg::Get { path } => {
                put_str(&mut body, path);
                TAG_GET
            }
            Msg::GetData(d) => {
                body.extend_from_slice(d);
                TAG_GET_DATA
            }
            Msg::GetEnd { size } => {
                body.extend_from_slice(&size.to_le_bytes());
                TAG_GET_END
            }
            Msg::Error(s) => {
                put_str(&mut body, s);
                TAG_ERROR
            }
            Msg::Ping => TAG_PING,
            Msg::Pong { version } => {
                put_str(&mut body, version);
                TAG_PONG
            }
            Msg::Sensors => TAG_SENSORS,
            Msg::SensorsReply { text } => {
                put_str(&mut body, text);
                TAG_SENSORS_REPLY
            }
            Msg::Stat => TAG_STAT,
            Msg::StatReply(s) => {
                put_stat(&mut body, s);
                TAG_STAT_REPLY
            }
            Msg::Traffic => TAG_TRAFFIC,
            Msg::TrafficReply(t) => {
                for v in [t.dma_to_card, t.dma_from_card, t.aperture_to_card, t.aperture_from_card] {
                    put_u64(&mut body, v);
                }
                TAG_TRAFFIC_REPLY
            }
        };
        debug_assert!(
            body.len() < MAX_FRAME,
            "{} frame of {} bytes exceeds MAX_FRAME",
            self.name(),
            body.len() + 1
        );
        let mut out = Vec::with_capacity(5 + body.len());
        out.extend_from_slice(&((1 + body.len()) as u32).to_le_bytes());
        out.push(tag);
        out.extend_from_slice(&body);
        out
    }

    /// Decode one frame body (tag plus fields, without the length prefix).
    fn decode_body(tag: u8, body: &[u8]) -> Result<Msg, DecodeError> {
        let mut r = Reader { buf: body, pos: 0 };
        let msg = match tag {
            TAG_EXEC => {
                let n = r.u16()? as usize;
                let mut argv = Vec::with_capacity(n.min(256));
                for _ in 0..n {
                    argv.push(r.str()?);
                }
                let n = r.u16()? as usize;
                let mut env = Vec::with_capacity(n.min(256));
                for _ in 0..n {
                    let k = r.str()?;
                    let v = r.str()?;
                    env.push((k, v));
                }
                let cwd = r.str()?;
                Msg::Exec {
                    argv,
                    env,
                    cwd: if cwd.is_empty() { None } else { Some(cwd) },
                }
            }
            TAG_STDIN => Msg::Stdin(r.rest()),
            TAG_STDIN_EOF => Msg::StdinEof,
            TAG_STDOUT => Msg::Stdout(r.rest()),
            TAG_STDERR => Msg::Stderr(r.rest()),
            TAG_EXIT => Msg::Exit(r.u32()? as i32),
            TAG_PUT_OPEN => {
                let path = r.str()?;
                let mode = r.u32()?;
                Msg::PutOpen { path, mode }
            }
            TAG_PUT_DATA => Msg::PutData(r.rest()),
            TAG_PUT_CLOSE => Msg::PutClose,
            TAG_GET => Msg::Get { path: r.str()? },
            TAG_GET_DATA => Msg::GetData(r.rest()),
            TAG_GET_END => Msg::GetEnd { size: r.u64()? },
            TAG_ERROR => Msg::Error(r.str()?),
            TAG_PING => Msg::Ping,
            TAG_PONG => Msg::Pong { version: r.str()? },
            TAG_SENSORS => Msg::Sensors,
            TAG_SENSORS_REPLY => Msg::SensorsReply { text: r.str()? },
            TAG_STAT => Msg::Stat,
            TAG_STAT_REPLY => Msg::StatReply(Box::new(r.stat()?)),
            TAG_TRAFFIC => Msg::Traffic,
            TAG_TRAFFIC_REPLY => Msg::TrafficReply(Traffic {
                dma_to_card: r.u64()?,
                dma_from_card: r.u64()?,
                aperture_to_card: r.u64()?,
                aperture_from_card: r.u64()?,
            }),
            t => return Err(DecodeError::Tag(t)),
        };
        Ok(msg)
    }
}

/// Incremental frame decoder over a byte stream.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append received bytes.
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Bytes buffered but not yet forming a whole frame.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }

    /// Take the next complete frame, if any. A frame that does not decode
    /// is discarded and reported, and the decoder stays usable: after
    /// [`DecodeError::Tag`] or [`DecodeError::Malformed`] exactly that
    /// frame is gone (its length was valid, so the stream is still in
    /// sync); after [`DecodeError::Length`] every buffered byte is gone,
    /// because the frame boundary is lost and only fresh input can restore
    /// it (the bytes of the oversize frame still in flight will likely be
    /// reported the same way).
    pub fn next_frame(&mut self) -> Result<Option<Msg>, DecodeError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]);
        if len == 0 || len as usize > MAX_FRAME {
            self.buf.clear();
            return Err(DecodeError::Length(len));
        }
        let total = 4 + len as usize;
        if self.buf.len() < total {
            return Ok(None);
        }
        let result = Msg::decode_body(self.buf[4], &self.buf[5..total]);
        self.buf.drain(..total);
        result.map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame with the given length field, tag and body bytes, as built
    /// by hand (so that it can be wrong).
    fn frame(len: u32, tag: u8, body: &[u8]) -> Vec<u8> {
        let mut f = len.to_le_bytes().to_vec();
        f.push(tag);
        f.extend_from_slice(body);
        f
    }

    fn decode_all(bytes: &[u8]) -> Vec<Result<Msg, DecodeError>> {
        let mut d = Decoder::new();
        d.push(bytes);
        let mut out = Vec::new();
        loop {
            match d.next_frame() {
                Ok(Some(m)) => out.push(Ok(m)),
                Ok(None) => break,
                Err(e) => out.push(Err(e)),
            }
        }
        out
    }

    /// One of every message, with non-trivial payloads.
    fn every_message() -> Vec<Msg> {
        vec![
            Msg::Exec {
                argv: vec!["/bin/sh".into(), "-c".into(), "echo hi".into()],
                env: vec![("PATH".into(), "/bin".into())],
                cwd: Some("/tmp".into()),
            },
            Msg::Exec {
                argv: vec![],
                env: vec![],
                cwd: None,
            },
            Msg::Stdin(b"abc".to_vec()),
            Msg::StdinEof,
            Msg::Stdout(vec![0, 255, 7]),
            Msg::Stderr(vec![]),
            Msg::Exit(-1),
            Msg::PutOpen {
                path: "/opt/x".into(),
                mode: 0o755,
            },
            Msg::PutData(vec![1; 1000]),
            Msg::PutClose,
            Msg::Get {
                path: "/etc/passwd".into(),
            },
            Msg::GetData(vec![2; 10]),
            Msg::GetEnd { size: 1 << 40 },
            Msg::Error("no such file".into()),
            Msg::Ping,
            Msg::Pong { version: "0.1".into() },
            Msg::Sensors,
            Msg::SensorsReply {
                text: "die temperatures: 54 C\n".into(),
            },
            Msg::Stat,
            Msg::StatReply(Box::new(sample_stat())),
            Msg::Traffic,
            Msg::TrafficReply(Traffic {
                dma_to_card: 1,
                dma_from_card: 2,
                aperture_to_card: 3,
                aperture_from_card: u64::MAX,
            }),
        ]
    }

    fn sample_stat() -> Stat {
        Stat {
            uptime_ms: 123_456,
            cpus: (0..228)
                .map(|i| CpuStat {
                    core: (i / 4) as u16,
                    busy: i * 7,
                    idle: 1_000_000 - i,
                })
                .collect(),
            mem: MemStat {
                total_kb: 5_805_088,
                avail_kb: 5_424_336,
                buffers_kb: 8612,
                cached_kb: 18808,
                swap_total_kb: 4_194_300,
                swap_free_kb: 4_194_300,
            },
            load: [133, 39, 14],
            running: 2,
            tasks: 1507,
            temps: vec![54, 53, 50, 46, 51, 46, 51, -1, -1, -1, -1, -1, -1, 0, -1],
            temp_peak: 66,
            vcore_mv: 1100,
            core_mhz: 1100,
            disks: vec![DevStat {
                name: "phiblk0".into(),
                read: 1 << 40,
                written: 512,
            }],
            nets: vec![DevStat {
                name: "phi0".into(),
                read: 866,
                written: 0,
            }],
            procs: vec![
                ProcStat {
                    pid: 1,
                    state: b'S',
                    kthread: false,
                    ticks: 145,
                    rss_kb: 556,
                    threads: 1,
                    comm: "init".into(),
                },
                ProcStat {
                    pid: 12,
                    state: b'R',
                    kthread: true,
                    ticks: 3,
                    rss_kb: 0,
                    threads: 1,
                    comm: "ksoftirqd/1".into(),
                },
            ],
            nprocs: 1507,
            nkthreads: 1490,
        }
    }

    #[test]
    fn round_trip_every_message() {
        let msgs = every_message();
        let mut stream = Vec::new();
        for m in &msgs {
            stream.extend_from_slice(&m.encode());
        }
        // Feed in odd-sized pieces to exercise reassembly.
        for piece_len in [1, 5, 7, 4096] {
            let mut d = Decoder::new();
            let mut got = Vec::new();
            for piece in stream.chunks(piece_len) {
                d.push(piece);
                while let Some(m) = d.next_frame().unwrap() {
                    got.push(m);
                }
            }
            assert_eq!(got, msgs);
            assert_eq!(d.pending(), 0);
        }
    }

    #[test]
    fn tags_are_stable() {
        // The wire tags are shared with every agent ever built; a renumbering
        // would only show up on the card.
        let expect: Vec<(u8, &str)> = vec![
            (1, "Exec"),
            (2, "Stdin"),
            (3, "StdinEof"),
            (4, "Stdout"),
            (5, "Stderr"),
            (6, "Exit"),
            (7, "PutOpen"),
            (8, "PutData"),
            (9, "PutClose"),
            (10, "Get"),
            (11, "GetData"),
            (12, "GetEnd"),
            (13, "Error"),
            (14, "Ping"),
            (15, "Pong"),
            (16, "Sensors"),
            (17, "SensorsReply"),
            (18, "Stat"),
            (19, "StatReply"),
            (20, "Traffic"),
            (21, "TrafficReply"),
        ];
        let mut msgs = every_message();
        msgs.remove(1); // the second Exec
        assert_eq!(msgs.len(), expect.len());
        for (m, (tag, name)) in msgs.iter().zip(&expect) {
            assert_eq!(m.encode()[4], *tag, "{name}");
            assert_eq!(m.name(), *name);
        }
    }

    #[test]
    fn rejects_bad_frames() {
        let mut d = Decoder::new();
        d.push(&[0, 0, 0, 0]);
        assert_eq!(d.next_frame(), Err(DecodeError::Length(0)));
        let mut d = Decoder::new();
        d.push(&[1, 0, 0, 0, 200]);
        assert_eq!(d.next_frame(), Err(DecodeError::Tag(200)));
        let mut d = Decoder::new();
        d.push(&[2, 0, 0, 0, TAG_GET, 5]);
        assert_eq!(d.next_frame(), Err(DecodeError::Malformed));
        // A string that is not UTF-8.
        let mut d = Decoder::new();
        d.push(&frame(4, TAG_ERROR, &[1, 0, 0xff]));
        assert_eq!(d.next_frame(), Err(DecodeError::Malformed));
    }

    #[test]
    fn every_truncated_body_is_malformed() {
        // Cutting any structured body short must be reported, never read as
        // a shorter valid message; a bytes body is valid at every length.
        for m in every_message() {
            let full = m.encode();
            let body = &full[5..];
            let bytes_body = matches!(
                m,
                Msg::Stdin(_) | Msg::Stdout(_) | Msg::Stderr(_) | Msg::PutData(_) | Msg::GetData(_)
            );
            for cut in 0..body.len() {
                let f = frame(1 + cut as u32, full[4], &body[..cut]);
                let got = decode_all(&f);
                assert_eq!(got.len(), 1, "{} cut at {cut}", m.name());
                if bytes_body {
                    assert!(got[0].is_ok(), "{} cut at {cut}", m.name());
                } else {
                    assert_eq!(got[0], Err(DecodeError::Malformed), "{} cut at {cut}", m.name());
                }
            }
        }
    }

    #[test]
    fn trailing_bytes_are_ignored() {
        let mut f = Msg::Exit(3).encode();
        f.extend_from_slice(&[9, 9, 9]);
        f[0] += 3;
        assert_eq!(decode_all(&f), vec![Ok(Msg::Exit(3))]);
    }

    #[test]
    fn bad_frame_keeps_sync_and_bad_length_resets() {
        // An unknown tag and a malformed body each cost exactly their own
        // frame: the frames around them still decode.
        let mut stream = Msg::Ping.encode();
        stream.extend_from_slice(&frame(3, 200, &[1, 2]));
        stream.extend_from_slice(&Msg::Exit(1).encode());
        stream.extend_from_slice(&frame(2, TAG_GET, &[5]));
        stream.extend_from_slice(&Msg::StdinEof.encode());
        assert_eq!(
            decode_all(&stream),
            vec![
                Ok(Msg::Ping),
                Err(DecodeError::Tag(200)),
                Ok(Msg::Exit(1)),
                Err(DecodeError::Malformed),
                Ok(Msg::StdinEof),
            ]
        );
        // A bad length loses everything buffered, and only that: what comes
        // in afterwards decodes again.
        let mut d = Decoder::new();
        let mut stream = frame(u32::MAX, TAG_PING, &[]);
        stream.extend_from_slice(&Msg::Ping.encode());
        d.push(&stream);
        assert_eq!(d.next_frame(), Err(DecodeError::Length(u32::MAX)));
        assert_eq!(d.pending(), 0);
        assert_eq!(d.next_frame(), Ok(None));
        d.push(&Msg::Exit(0).encode());
        assert_eq!(d.next_frame(), Ok(Some(Msg::Exit(0))));
    }

    #[test]
    fn frame_at_the_limit() {
        // The length field may be exactly MAX_FRAME: a tag and a body of
        // MAX_FRAME - 1 bytes.
        let m = Msg::Stdin(vec![0xab; MAX_FRAME - 1]);
        let f = m.encode();
        assert_eq!(f.len(), 4 + MAX_FRAME);
        assert_eq!(u32::from_le_bytes([f[0], f[1], f[2], f[3]]) as usize, MAX_FRAME);
        let mut d = Decoder::new();
        d.push(&f[..1000]);
        assert_eq!(d.next_frame(), Ok(None));
        d.push(&f[1000..]);
        assert_eq!(d.next_frame(), Ok(Some(m)));
        // One byte more is refused before the body is even read.
        let mut d = Decoder::new();
        d.push(&(MAX_FRAME as u32 + 1).to_le_bytes());
        assert_eq!(d.next_frame(), Err(DecodeError::Length(MAX_FRAME as u32 + 1)));
    }

    #[test]
    fn strings_and_lists_are_clipped_at_their_limits() {
        // Exactly the limit passes whole.
        let s = "a".repeat(MAX_STRING);
        let m = Msg::Error(s.clone());
        assert_eq!(decode_all(&m.encode()), vec![Ok(m)]);
        // One more byte is cut, and the cut lands on a character boundary
        // so the receiver still gets valid UTF-8: here the two-byte "é"
        // straddling the limit goes entirely.
        let mut s = "a".repeat(MAX_STRING - 1);
        s.push('\u{e9}');
        assert_eq!(s.len(), MAX_STRING + 1);
        let m = Msg::Error(s);
        assert_eq!(decode_all(&m.encode()), vec![Ok(Msg::Error("a".repeat(MAX_STRING - 1)))]);
        assert_eq!(clip("h\u{e9}llo", 2), "h");
        assert_eq!(clip("h\u{e9}llo", 3), "h\u{e9}");
        assert_eq!(clip("", 0), "");
        // A list of exactly MAX_LIST items round-trips; one more is dropped.
        let argv: Vec<String> = (0..MAX_LIST + 1).map(|i| (i % 10).to_string()).collect();
        let m = Msg::Exec {
            argv: argv.clone(),
            env: vec![],
            cwd: None,
        };
        match decode_all(&m.encode()).remove(0) {
            Ok(Msg::Exec { argv: got, .. }) => assert_eq!(got, argv[..MAX_LIST]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn empty_cwd_is_none() {
        let m = Msg::Exec {
            argv: vec!["x".into()],
            env: vec![],
            cwd: Some(String::new()),
        };
        match decode_all(&m.encode()).remove(0) {
            Ok(Msg::Exec { cwd, .. }) => assert_eq!(cwd, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn stat_lists_at_their_limits() {
        // Stat lists longer than the wire allows keep their first MAX_LIST
        // entries and the rest of the sample still decodes. (The process
        // list is bounded by the frame before the count: at 28 bytes plus
        // the name per entry, MAX_LIST of them would not fit MAX_FRAME; the
        // agent caps what it sends, see card/agent/src/stat.rs.)
        let mut s = sample_stat();
        s.cpus = (0..MAX_LIST as u32 + 5)
            .map(|i| CpuStat {
                core: (i / 4) as u16,
                busy: i,
                idle: 0,
            })
            .collect();
        s.temps = vec![-1; MAX_LIST + 1];
        let m = Msg::StatReply(Box::new(s));
        match decode_all(&m.encode()).remove(0) {
            Ok(Msg::StatReply(got)) => {
                assert_eq!(got.cpus.len(), MAX_LIST);
                assert_eq!(got.cpus[MAX_LIST - 1].busy, MAX_LIST as u32 - 1);
                assert_eq!(got.temps.len(), MAX_LIST);
                assert_eq!(got.nprocs, 1507);
                assert_eq!(got.procs.len(), 2);
            }
            other => panic!("{other:?}"),
        }
    }
}
