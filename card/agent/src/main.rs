//! phi-agent: the card end of `phictl exec`, `put`, `get` and `status`.
//!
//! Reads rpc frames (`phi-rpc`) from `/dev/phirpc` (kernel patch 0022), a
//! byte stream from the host's `phictl boot --serve`, and answers them one
//! session at a time: a command runs with its standard streams relayed,
//! a file is written or read back. Runs as root on a RAM-only system that
//! the host can reset at will; the trust boundary is the host daemon's
//! socket, not this program. See main.md.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use phi_rpc::{Decoder, Msg};

const DEVICE: &str = "/dev/phirpc";
/// Bytes per Stdout/Stderr/GetData frame.
const CHUNK: usize = 32768;

/// The device, shared between the reader (main thread) and the output
/// pumps; frames are written whole under the lock so they never interleave.
#[derive(Clone)]
struct Link(Arc<Mutex<File>>);

impl Link {
    fn send(&self, m: &Msg) -> io::Result<()> {
        let mut f = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f.write_all(&m.encode())
    }
}

/// Copy a child's stream to the host as frames until EOF, or until the
/// session is abandoned (a daemonised grandchild keeping the pipe open must
/// not keep the session open too; its later output is discarded).
fn pump(mut src: impl Read + Send + 'static, link: Link, wrap: fn(Vec<u8>) -> Msg, abandoned: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buf = vec![0u8; CHUNK];
        loop {
            match src.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if abandoned.load(Ordering::Acquire) || link.send(&wrap(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    })
}

/// How long output pumps may run on after the command itself has exited.
const OUTPUT_GRACE: Duration = Duration::from_millis(300);

fn exit_code(child: &mut Child) -> i32 {
    match child.wait() {
        Ok(st) => st.code().unwrap_or_else(|| 128 + st.signal().unwrap_or(0)),
        Err(_) => 255,
    }
}

/// Run one command: relay Stdin frames until StdinEof or the command ends;
/// return once Exit has been sent.
fn run_exec(reader: &mut Reader, link: &Link, argv: Vec<String>, env: Vec<(String, String)>, cwd: Option<String>) -> io::Result<()> {
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
    let out = pump(child.stdout.take().expect("piped"), link.clone(), Msg::Stdout, abandoned.clone());
    let err = pump(child.stderr.take().expect("piped"), link.clone(), Msg::Stderr, abandoned.clone());
    let mut stdin = child.stdin.take();
    // Feed input while the command runs. A command that exits with input
    // still pending simply stops reading; the host learns from Exit.
    loop {
        if stdin.is_none() {
            break;
        }
        if child.try_wait()?.is_some() {
            break;
        }
        match reader.next_timeout(50)? {
            Some(Msg::Stdin(d)) => {
                if let Some(s) = stdin.as_mut() {
                    if s.write_all(&d).is_err() {
                        stdin = None;
                    }
                }
            }
            Some(Msg::StdinEof) => stdin = None,
            Some(other) => {
                let _ = link.send(&Msg::Error(format!("unexpected {other:?} during a command")));
            }
            None => {}
        }
    }
    drop(stdin);
    let code = exit_code(&mut child);
    // Let the pumps drain what the command left in its pipes, then give up
    // on them: a background process it started may hold the pipes open.
    let deadline = Instant::now() + OUTPUT_GRACE;
    for h in [out, err] {
        while !h.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
    }
    abandoned.store(true, Ordering::Release);
    link.send(&Msg::Exit(code))
}

fn run_put(reader: &mut Reader, link: &Link, path: String, mode: u32) -> io::Result<()> {
    let mut file = match OpenOptions::new().write(true).create(true).truncate(true).mode(mode).open(&path) {
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
            other => return link.send(&Msg::Error(format!("unexpected {other:?} during put"))),
        }
    }
}

fn run_get(link: &Link, path: String) -> io::Result<()> {
    let mut file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => return link.send(&Msg::Error(format!("{path}: {e}"))),
    };
    let mut buf = vec![0u8; CHUNK];
    let mut size = 0u64;
    loop {
        match file.read(&mut buf) {
            Ok(0) => return link.send(&Msg::GetEnd { size }),
            Ok(n) => {
                link.send(&Msg::GetData(buf[..n].to_vec()))?;
                size += n as u64;
            }
            Err(e) => return link.send(&Msg::Error(format!("{path}: {e}"))),
        }
    }
}

/// Frame reader over the device with an optional wait bound (poll).
struct Reader {
    file: File,
    dec: Decoder,
    buf: Vec<u8>,
}

impl Reader {
    fn fill(&mut self) -> io::Result<usize> {
        let n = self.file.read(&mut self.buf)?;
        self.dec.push(&self.buf[..n]);
        Ok(n)
    }

    fn next_blocking(&mut self) -> io::Result<Msg> {
        loop {
            match self.dec.next_frame() {
                Ok(Some(m)) => return Ok(m),
                Ok(None) => {
                    if self.fill()? == 0 {
                        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "device closed"));
                    }
                }
                Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
            }
        }
    }

    /// Next frame if one arrives within `ms` milliseconds.
    fn next_timeout(&mut self, ms: i32) -> io::Result<Option<Msg>> {
        if let Some(m) = self.dec.next_frame().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))? {
            return Ok(Some(m));
        }
        let mut pfd = libc::pollfd {
            fd: std::os::fd::AsRawFd::as_raw_fd(&self.file),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one pollfd valid for the call.
        let ready = unsafe { libc::poll(&mut pfd, 1, ms) };
        if ready > 0 && pfd.revents & libc::POLLIN != 0 {
            self.fill()?;
        }
        self.dec.next_frame().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
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
    eprintln!("phi-agent {} listening on {DEVICE}", env!("CARGO_PKG_VERSION"));
    loop {
        let result = match reader.next_blocking() {
            Ok(Msg::Exec { argv, env, cwd }) => run_exec(&mut reader, &link, argv, env, cwd),
            Ok(Msg::PutOpen { path, mode }) => run_put(&mut reader, &link, path, mode),
            Ok(Msg::Get { path }) => run_get(&link, path),
            Ok(Msg::Ping) => link.send(&Msg::Pong { version: env!("CARGO_PKG_VERSION").into() }),
            Ok(other) => link.send(&Msg::Error(format!("unexpected {other:?} outside a session"))),
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                eprintln!("phi-agent: {e}; resetting the frame decoder");
                reader.dec = Decoder::new();
                Ok(())
            }
            Err(e) => {
                eprintln!("phi-agent: {DEVICE}: {e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = result {
            eprintln!("phi-agent: {e}");
            std::process::exit(1);
        }
    }
}
