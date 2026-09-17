//! phi-agent: the card end of `phictl exec`, `put`, `get`, `status` and of
//! `phitop`'s samples.
//!
//! Reads rpc frames (`phi-rpc`) from `/dev/phirpc` (kernel patch 0022), a
//! byte stream from the host's `phictl boot --serve`, and answers them one
//! session at a time: a command runs with its standard streams relayed,
//! a file is written or read back; `Stat` is answered from every state.
//! Runs as root on a RAM-only system that the host can reset at will; the
//! trust boundary is the host daemon's socket, not this program. Started
//! once by `/init`, without respawn, so nothing the host sends may end it:
//! only the device failing does. See main.md.

use std::fs::{File, OpenOptions, Permissions};
use std::io::{self, ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use phi_rpc::{Decoder, Msg};

mod stat;

const DEVICE: &str = "/dev/phirpc";
/// Bytes per Stdout/Stderr/GetData frame.
const CHUNK: usize = 32768;
/// How long the command loop waits for a frame before looking at the child
/// again (and at its input pipe, when input is queued).
const CHILD_POLL_MS: i32 = 20;
/// How long output pumps may run on after the command itself has exited.
const OUTPUT_GRACE: Duration = Duration::from_millis(300);
/// How often an output pump waiting on a silent pipe re-checks whether its
/// session has been abandoned.
const PUMP_POLL_MS: i32 = 100;
/// Command input queued beyond this stops the device from being read until
/// the command has taken some of it: the backlog then builds in the ring
/// and the host's buffers, not in the card's memory.
const INPUT_BACKLOG: usize = 1 << 20;

/// The device, shared between the reader (main thread) and the output
/// pumps; frames are written whole under the lock so they never interleave.
#[derive(Clone)]
struct Link(Arc<Mutex<File>>);

impl Link {
    fn lock(&self) -> MutexGuard<'_, File> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn send(&self, m: &Msg) -> io::Result<()> {
        self.lock().write_all(&m.encode())
    }

    /// Send unless the session has been abandoned. The flag is read under
    /// the lock that `end` sets it under, so a frame sent this way can
    /// never follow the session's last frame; returns whether it was sent.
    fn send_live(&self, m: &Msg, abandoned: &AtomicBool) -> io::Result<bool> {
        let mut f = self.lock();
        if abandoned.load(Ordering::Relaxed) {
            return Ok(false);
        }
        f.write_all(&m.encode())?;
        Ok(true)
    }

    /// Abandon the session's pumps and send its last frame as one step.
    fn end(&self, abandoned: &AtomicBool, last: &Msg) -> io::Result<()> {
        let mut f = self.lock();
        abandoned.store(true, Ordering::Relaxed);
        f.write_all(&last.encode())
    }
}

fn pollfd(fd: RawFd, events: i16) -> libc::pollfd {
    libc::pollfd {
        fd,
        events,
        revents: 0,
    }
}

/// poll(2) on `fds` for at most `ms` milliseconds; entries with a negative
/// fd are skipped by the kernel. Returns how many are ready (EINTR counts
/// as none).
fn poll(fds: &mut [libc::pollfd], ms: i32) -> usize {
    // SAFETY: `fds` is a valid, exclusively borrowed slice for the call.
    let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
    n.max(0) as usize
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl on a descriptor this process owns; no pointers.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Copy a child's stream to the host as frames until EOF, or until the
/// session is abandoned: a daemonised grandchild keeping the pipe open must
/// not keep the session open too, nor keep this thread alive, so the pump
/// waits for data with a timeout and re-checks the flag, and its later
/// output is discarded.
fn pump<R: Read + AsRawFd + Send + 'static>(
    mut src: R,
    link: Link,
    wrap: fn(Vec<u8>) -> Msg,
    abandoned: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buf = vec![0u8; CHUNK];
        let mut fds = [pollfd(src.as_raw_fd(), libc::POLLIN)];
        while !abandoned.load(Ordering::Relaxed) {
            if poll(&mut fds, PUMP_POLL_MS) == 0 {
                continue;
            }
            match src.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => match link.send_live(&wrap(buf[..n].to_vec()), &abandoned) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => break,
                },
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    })
}

/// A command's standard input: what the host sent that the pipe has not
/// taken yet. The pipe is non-blocking, so a command that is slow to read
/// (or never reads) cannot stall the frame loop and with it the `Stat`
/// replies; the bytes wait here, up to `INPUT_BACKLOG`.
struct Input {
    pipe: Option<ChildStdin>,
    pending: Vec<u8>,
    /// Bytes of `pending` already written.
    head: usize,
    /// `StdinEof` has arrived: close the pipe once `pending` is drained.
    eof: bool,
}

impl Input {
    fn new(pipe: ChildStdin) -> Self {
        if let Err(e) = set_nonblocking(pipe.as_raw_fd()) {
            // Writes then block until the command reads; only the frame
            // loop's responsiveness is lost, not the input.
            eprintln!("phi-agent: stdin pipe: {e}");
        }
        Self {
            pipe: Some(pipe),
            pending: Vec::new(),
            head: 0,
            eof: false,
        }
    }

    /// Bytes waiting for the pipe.
    fn queued(&self) -> usize {
        self.pending.len() - self.head
    }

    fn push(&mut self, data: &[u8]) {
        if self.pipe.is_some() && !self.eof {
            self.pending.extend_from_slice(data);
        }
    }

    fn close_when_drained(&mut self) {
        self.eof = true;
    }

    /// Write what the pipe takes now. The pipe is closed once everything
    /// has been written after `StdinEof`, or as soon as it breaks (the
    /// command closed its end or exited: the rest of the input is moot).
    fn flush(&mut self) {
        let Some(pipe) = self.pipe.as_mut() else {
            return;
        };
        while self.head < self.pending.len() {
            match pipe.write(&self.pending[self.head..]) {
                Ok(0) => break,
                Ok(n) => self.head += n,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => {
                    self.pipe = None;
                    self.pending.clear();
                    self.head = 0;
                    return;
                }
            }
        }
        if self.head == self.pending.len() {
            self.pending.clear();
            self.head = 0;
            if self.eof {
                self.pipe = None;
            }
        } else if self.head >= self.pending.len() / 2 {
            self.pending.drain(..self.head);
            self.head = 0;
        }
    }

    /// The pipe as a poll entry, while it has something to take.
    fn pollfd(&self) -> Option<libc::pollfd> {
        self.pipe
            .as_ref()
            .filter(|_| self.queued() > 0)
            .map(|p| pollfd(p.as_raw_fd(), libc::POLLOUT))
    }
}

fn exit_code(child: &mut Child) -> i32 {
    match child.wait() {
        Ok(st) => st.code().unwrap_or_else(|| 128 + st.signal().unwrap_or(0)),
        Err(_) => 255,
    }
}

fn stat_reply(sampler: &mut stat::Sampler) -> Msg {
    Msg::StatReply(Box::new(sampler.sample()))
}

/// Run one command: relay `Stdin` frames until the command ends, answer
/// `Stat` meanwhile; return once `Exit` has been sent.
fn run_exec(
    reader: &mut Reader,
    link: &Link,
    sampler: &mut stat::Sampler,
    argv: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<String>,
) -> io::Result<()> {
    let Some(program) = argv.first() else {
        return link.send(&Msg::Error("empty argv".into()));
    };
    let mut cmd = Command::new(program);
    cmd.args(&argv[1..])
        .env_clear()
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return link.send(&Msg::Error(format!("{program}: {e}"))),
    };
    let abandoned = Arc::new(AtomicBool::new(false));
    let out = pump(
        child.stdout.take().expect("piped"),
        link.clone(),
        Msg::Stdout,
        abandoned.clone(),
    );
    let err = pump(
        child.stderr.take().expect("piped"),
        link.clone(),
        Msg::Stderr,
        abandoned.clone(),
    );
    let mut input = Input::new(child.stdin.take().expect("piped"));
    // Feed input while the command runs, and keep reading frames after the
    // host closed its input: Stat requests arrive at any time and a command
    // may run for hours. A command that exits with input still pending
    // simply stops reading; the host learns from Exit. Any other frame here
    // is a client's mistake and is dropped: an Error reply would tell the
    // daemon the session is over while the command runs on.
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        input.flush();
        let read_device = input.queued() < INPUT_BACKLOG;
        match reader.next_timeout(CHILD_POLL_MS, input.pollfd(), read_device)? {
            Some(Msg::Stdin(d)) => input.push(&d),
            Some(Msg::StdinEof) => input.close_when_drained(),
            Some(Msg::Stat) => link.send(&stat_reply(sampler))?,
            Some(other) => eprintln!("phi-agent: dropping {} during a command", other.name()),
            None => {}
        }
    }
    drop(input);
    let code = exit_code(&mut child);
    // Let the pumps drain what the command left in its pipes, then give up
    // on them: a background process it started may hold the pipes open.
    let deadline = Instant::now() + OUTPUT_GRACE;
    for h in [out, err] {
        while !h.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
    }
    link.end(&abandoned, &Msg::Exit(code))
}

/// Receive one file: `PutData` frames until `PutClose`, `Stat` answered
/// meanwhile; `Exit(0)` once the file is on disk, or `Error`.
fn run_put(
    reader: &mut Reader,
    link: &Link,
    sampler: &mut stat::Sampler,
    path: String,
    mode: u32,
) -> io::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(&path)
        // open(2) applies the mode only to a file it creates, and after the
        // umask; the host asked for exactly this mode.
        .and_then(|f| f.set_permissions(Permissions::from_mode(mode)).map(|()| f));
    let mut file = match file {
        Ok(f) => f,
        Err(e) => return link.send(&Msg::Error(format!("{path}: {e}"))),
    };
    loop {
        match reader.next_blocking()? {
            Msg::PutData(d) => {
                if let Err(e) = file.write_all(&d) {
                    return link.send(&Msg::Error(format!("{path}: {e}")));
                }
            }
            Msg::PutClose => {
                return match file.sync_all() {
                    Ok(()) => link.send(&Msg::Exit(0)),
                    Err(e) => link.send(&Msg::Error(format!("{path}: {e}"))),
                };
            }
            Msg::Stat => link.send(&stat_reply(sampler))?,
            other => eprintln!("phi-agent: dropping {} during a put", other.name()),
        }
    }
}

/// Send one file as `GetData` frames, then `GetEnd`; `Stat` requests that
/// arrive meanwhile are answered between chunks.
fn run_get(
    reader: &mut Reader,
    link: &Link,
    sampler: &mut stat::Sampler,
    path: String,
) -> io::Result<()> {
    let mut file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => return link.send(&Msg::Error(format!("{path}: {e}"))),
    };
    let mut buf = vec![0u8; CHUNK];
    let mut size = 0u64;
    loop {
        while let Some(m) = reader.next_timeout(0, None, true)? {
            match m {
                Msg::Stat => link.send(&stat_reply(sampler))?,
                other => eprintln!("phi-agent: dropping {} during a get", other.name()),
            }
        }
        match file.read(&mut buf) {
            Ok(0) => return link.send(&Msg::GetEnd { size }),
            Ok(n) => {
                link.send(&Msg::GetData(buf[..n].to_vec()))?;
                size += n as u64;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return link.send(&Msg::Error(format!("{path}: {e}"))),
        }
    }
}

/// Frame reader over the device. Frames that do not decode are dropped
/// here, as the decoder defines it (`phi_rpc::Decoder::next_frame`), and
/// logged; the callers only ever see whole messages or a device failure.
struct Reader {
    file: File,
    dec: Decoder,
    buf: Vec<u8>,
}

impl Reader {
    /// One read from the device into the decoder; 0 means the device is
    /// closed.
    fn fill(&mut self) -> io::Result<usize> {
        loop {
            match self.file.read(&mut self.buf) {
                Ok(n) => {
                    self.dec.push(&self.buf[..n]);
                    return Ok(n);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// The next whole frame among the bytes already read, if any.
    fn buffered(&mut self) -> Option<Msg> {
        loop {
            match self.dec.next_frame() {
                Ok(m) => return m,
                Err(e) => eprintln!("phi-agent: bad frame from the host: {e}"),
            }
        }
    }

    fn next_blocking(&mut self) -> io::Result<Msg> {
        loop {
            if let Some(m) = self.buffered() {
                return Ok(m);
            }
            if self.fill()? == 0 {
                return Err(io::Error::new(ErrorKind::UnexpectedEof, "device closed"));
            }
        }
    }

    /// Next frame if one is already buffered or arrives within `ms`
    /// milliseconds. `also` is polled alongside the device (the caller acts
    /// on it afterwards); with `read_device` false the device is not polled
    /// at all, so nothing new is read until the caller allows it again.
    fn next_timeout(
        &mut self,
        ms: i32,
        also: Option<libc::pollfd>,
        read_device: bool,
    ) -> io::Result<Option<Msg>> {
        if let Some(m) = self.buffered() {
            return Ok(Some(m));
        }
        let device = if read_device {
            self.file.as_raw_fd()
        } else {
            -1
        };
        let mut fds = [pollfd(device, libc::POLLIN), also.unwrap_or(pollfd(-1, 0))];
        if poll(&mut fds, ms) == 0 {
            return Ok(None);
        }
        if fds[0].revents & libc::POLLIN != 0 && self.fill()? == 0 {
            return Err(io::Error::new(ErrorKind::UnexpectedEof, "device closed"));
        }
        Ok(self.buffered())
    }
}

fn main() {
    let file = match OpenOptions::new().read(true).write(true).open(DEVICE) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("phi-agent: {DEVICE}: {e}");
            std::process::exit(1);
        }
    };
    let writer = match file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("phi-agent: {e}");
            std::process::exit(1);
        }
    };
    let link = Link(Arc::new(Mutex::new(writer)));
    let mut reader = Reader {
        file,
        dec: Decoder::new(),
        buf: vec![0u8; 65536],
    };
    eprintln!(
        "phi-agent {} listening on {DEVICE}",
        env!("CARGO_PKG_VERSION")
    );
    let mut sampler = stat::Sampler::default();
    loop {
        let result = match reader.next_blocking() {
            Ok(Msg::Exec { argv, env, cwd }) => {
                run_exec(&mut reader, &link, &mut sampler, argv, env, cwd)
            }
            Ok(Msg::PutOpen { path, mode }) => {
                run_put(&mut reader, &link, &mut sampler, path, mode)
            }
            Ok(Msg::Get { path }) => run_get(&mut reader, &link, &mut sampler, path),
            Ok(Msg::Ping) => link.send(&Msg::Pong {
                version: env!("CARGO_PKG_VERSION").into(),
            }),
            Ok(Msg::Stat) => link.send(&stat_reply(&mut sampler)),
            // Stream frames that outlived their session (the daemon closes a
            // departed client's input after the fact, and a command may exit
            // before its input has all arrived): nothing to answer.
            Ok(Msg::Stdin(_) | Msg::StdinEof | Msg::PutData(_) | Msg::PutClose) => Ok(()),
            // Card-to-host and host-only messages have no business here; the
            // daemon reads the Error as the end of whatever it forwarded.
            Ok(other) => link.send(&Msg::Error(format!(
                "unexpected {} outside a session",
                other.name()
            ))),
            Err(e) => {
                eprintln!("phi-agent: {DEVICE}: {e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = result {
            eprintln!("phi-agent: {DEVICE}: {e}");
            std::process::exit(1);
        }
    }
}
