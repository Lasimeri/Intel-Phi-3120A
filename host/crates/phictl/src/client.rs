//! Client side of the control socket: `phictl exec`, `put`, `get`, `status`.
//!
//! Each command opens the socket, sends one request, relays the reply
//! frames, and returns the card's outcome; no privileges are needed beyond
//! being the socket's owner. See client.md.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;

use anyhow::{bail, Context, Result};
use phi_rpc::{Decoder, Msg};

/// Environment given to every command on the card.
const CARD_ENV: [(&str, &str); 3] = [
    ("PATH", "/opt/phi/bin:/bin:/sbin:/usr/bin:/usr/sbin"),
    ("HOME", "/root"),
    ("TERM", "dumb"),
];

struct Conn {
    stream: UnixStream,
    dec: Decoder,
    buf: Vec<u8>,
}

impl Conn {
    fn open(socket: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket).with_context(|| {
            format!(
                "connecting to {} (is `phictl boot --serve` running, and are you its --owner?)",
                socket.display()
            )
        })?;
        Ok(Self {
            stream,
            dec: Decoder::new(),
            buf: vec![0u8; 65536],
        })
    }

    fn send(&mut self, m: &Msg) -> Result<()> {
        self.stream.write_all(&m.encode()).context("writing to the control socket")
    }

    fn recv(&mut self) -> Result<Msg> {
        loop {
            if let Some(m) = self.dec.next_frame()? {
                return Ok(m);
            }
            let n = self.stream.read(&mut self.buf).context("reading the control socket")?;
            if n == 0 {
                bail!("the daemon closed the connection");
            }
            self.dec.push(&self.buf[..n]);
        }
    }
}

/// Run `argv` on the card; local stdin, stdout and stderr are relayed and
/// the card's exit status is returned.
pub fn exec(socket: &Path, cwd: Option<String>, argv: Vec<String>) -> Result<i32> {
    let mut conn = Conn::open(socket)?;
    conn.send(&Msg::Exec {
        argv,
        env: CARD_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        cwd,
    })?;
    let mut writer = conn.stream.try_clone()?;
    thread::spawn(move || {
        let mut buf = [0u8; 16384];
        let stdin = io::stdin();
        loop {
            match stdin.lock().read(&mut buf) {
                Ok(0) | Err(_) => {
                    let _ = writer.write_all(&Msg::StdinEof.encode());
                    break;
                }
                Ok(n) => {
                    if writer.write_all(&Msg::Stdin(buf[..n].to_vec()).encode()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    loop {
        match conn.recv()? {
            Msg::Stdout(d) => {
                out.write_all(&d)?;
                out.flush()?;
            }
            Msg::Stderr(d) => {
                err.write_all(&d)?;
                err.flush()?;
            }
            Msg::Exit(code) => return Ok(code),
            Msg::Error(e) => {
                eprintln!("phictl exec: {e}");
                return Ok(255);
            }
            other => bail!("unexpected reply {other:?}"),
        }
    }
}

/// Copy a local file to `dst` on the card, with the local mode unless given.
pub fn put(socket: &Path, src: &Path, dst: String, mode: Option<u32>) -> Result<()> {
    let mut file = File::open(src).with_context(|| format!("opening {}", src.display()))?;
    let mode = match mode {
        Some(m) => m,
        None => file.metadata()?.permissions().mode() & 0o7777,
    };
    let mut conn = Conn::open(socket)?;
    conn.send(&Msg::PutOpen { path: dst, mode })?;
    let mut buf = vec![0u8; 65536];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        conn.send(&Msg::PutData(buf[..n].to_vec()))?;
        total += n as u64;
    }
    conn.send(&Msg::PutClose)?;
    match conn.recv()? {
        Msg::Exit(0) => {
            eprintln!("phictl put: {total} bytes");
            Ok(())
        }
        Msg::Exit(c) => bail!("card reported status {c}"),
        Msg::Error(e) => bail!("{e}"),
        other => bail!("unexpected reply {other:?}"),
    }
}

/// Copy `src` on the card to a local file.
pub fn get(socket: &Path, src: String, dst: &Path) -> Result<()> {
    let mut conn = Conn::open(socket)?;
    conn.send(&Msg::Get { path: src })?;
    let mut file: Option<File> = None;
    loop {
        match conn.recv()? {
            Msg::GetData(d) => {
                let f = match file.as_mut() {
                    Some(f) => f,
                    None => file.insert(
                        OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .mode(0o644)
                            .open(dst)
                            .with_context(|| format!("creating {}", dst.display()))?,
                    ),
                };
                f.write_all(&d)?;
            }
            Msg::GetEnd { size } => {
                if file.is_none() {
                    File::create(dst)?;
                }
                eprintln!("phictl get: {size} bytes");
                return Ok(());
            }
            Msg::Error(e) => bail!("{e}"),
            other => bail!("unexpected reply {other:?}"),
        }
    }
}

/// Ask the agent for its version.
pub fn status(socket: &Path) -> Result<()> {
    let mut conn = Conn::open(socket)?;
    conn.send(&Msg::Ping)?;
    match conn.recv()? {
        Msg::Pong { version } => {
            println!("card agent {version} reachable through {}", socket.display());
            Ok(())
        }
        Msg::Error(e) => bail!("{e}"),
        other => bail!("unexpected reply {other:?}"),
    }
}
