//! phitop: a resource viewer for the card in the spirit of glances, run on
//! the host. Every interval it asks the daemon (`phictl boot --serve`) for
//! a `Stat` sample, which the card's agent answers from /proc and sysfs,
//! and for the daemon's own PCIe byte counters (`Traffic`), then draws the
//! load of every hardware thread on a core grid, the die temperatures,
//! memory and swap, PCIe rates by path and direction, the card's disk and
//! network rates, and its processes. See main.md.

mod model;
mod term;
mod view;

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Parser;
use phi_rpc::{Decoder, Msg};

use model::{Derived, Model, Snapshot};
use view::Options;

/// Where a root daemon binds (as `phictl serve` does).
const ROOT_SOCKET: &str = "/run/phictl/control.sock";

#[derive(Parser)]
#[command(about = "Resource viewer for the Xeon Phi 3120A, from the host")]
struct Args {
    /// Seconds between samples.
    #[arg(short, long, default_value_t = 1.0)]
    interval: f64,
    /// Print this many frames as plain text and exit (no terminal control).
    #[arg(short, long)]
    batch: Option<u32>,
    /// The daemon's control socket (default: PHICTL_SOCKET, else the runtime directory).
    #[arg(long)]
    socket: Option<PathBuf>,
    /// Show the kernel threads the card reports (those that used CPU time).
    #[arg(short, long)]
    kthreads: bool,
}

/// The same lookup as `phictl` for a client: `PHICTL_SOCKET`, a root
/// daemon's socket when one answers, else the user's runtime directory.
fn default_socket() -> PathBuf {
    if let Ok(p) = std::env::var("PHICTL_SOCKET") {
        return PathBuf::from(p);
    }
    // SAFETY: geteuid and getuid have no preconditions.
    let (root, uid) = unsafe { (libc::geteuid() == 0, libc::getuid()) };
    if root || UnixStream::connect(ROOT_SOCKET).is_ok() {
        return PathBuf::from(ROOT_SOCKET);
    }
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(format!("/tmp/phictl-{uid}")))
        .join("phictl")
        .join("control.sock")
}

/// One request and its reply on a fresh connection, so that the daemon
/// can interleave it with whatever session another client holds.
fn request(socket: &Path, m: &Msg, timeout: Duration) -> Result<Msg> {
    let mut stream =
        UnixStream::connect(socket).with_context(|| format!("connecting to {} (is phictl boot --serve running?)", socket.display()))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.write_all(&m.encode())?;
    let mut dec = Decoder::new();
    let mut buf = vec![0u8; 65536];
    loop {
        if let Some(m) = dec.next_frame()? {
            return Ok(m);
        }
        let n = stream.read(&mut buf).context("waiting for the daemon's reply")?;
        if n == 0 {
            bail!("the daemon closed the connection");
        }
        dec.push(&buf[..n]);
    }
}

fn sample(socket: &Path) -> Result<Snapshot> {
    let stat = match request(socket, &Msg::Stat, Duration::from_secs(5))? {
        Msg::StatReply(s) => *s,
        Msg::Error(e) => bail!("card: {e}"),
        other => bail!("unexpected reply {other:?}"),
    };
    let traffic = match request(socket, &Msg::Traffic, Duration::from_secs(2))? {
        Msg::TrafficReply(t) => t,
        other => bail!("unexpected reply {other:?}"),
    };
    Ok(Snapshot {
        stat,
        traffic,
        at: Instant::now(),
    })
}

fn draw(model: &Model, d: Option<&Derived>, o: &Options) {
    let (cols, rows) = term::Term::size();
    let frame = match (model.current(), d) {
        (Some(cur), Some(d)) => view::render(cur, d, o, cols, rows),
        (Some(_), None) => "\x1b[Hsampling...\x1b[K".into(),
        (None, _) => format!("\x1b[H{}\x1b[K", o.error.as_deref().unwrap_or("connecting...")),
    };
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(frame.as_bytes());
    let _ = out.flush();
}

fn main() -> Result<()> {
    let args = Args::parse();
    let socket = args.socket.unwrap_or_else(default_socket);
    let mut opts = Options {
        sort_mem: false,
        show_kthreads: args.kthreads,
        color: false,
        interval_s: args.interval.clamp(0.2, 60.0),
        error: None,
    };
    let mut model = Model::default();
    if let Some(frames) = args.batch {
        let mut printed = 0;
        while printed < frames {
            match sample(&socket) {
                Ok(s) => {
                    if let Some(d) = model.update(s) {
                        let cur = model.current().expect("a sample was just stored");
                        println!("{}", view::render(cur, &d, &opts, 132, 32));
                        printed += 1;
                    }
                }
                Err(e) => eprintln!("phitop: {e:#}"),
            }
            std::thread::sleep(Duration::from_secs_f64(opts.interval_s));
        }
        return Ok(());
    }
    let term = term::Term::open().context("standard input is not a terminal (use --batch N for plain text)")?;
    opts.color = true;
    let mut derived: Option<Derived> = None;
    let mut next = Instant::now();
    loop {
        match sample(&socket) {
            Ok(s) => {
                opts.error = None;
                if let Some(d) = model.update(s) {
                    derived = Some(d);
                }
            }
            Err(e) => opts.error = Some(format!("{e:#}")),
        }
        draw(&model, derived.as_ref(), &opts);
        next += Duration::from_secs_f64(opts.interval_s);
        if next < Instant::now() {
            next = Instant::now();
        }
        loop {
            if term::STOP.load(Ordering::SeqCst) {
                drop(term);
                return Ok(());
            }
            let now = Instant::now();
            if now >= next {
                break;
            }
            let ms = (next - now).as_millis().min(500) as i32;
            let redraw = match term::Term::key(ms) {
                Some(b'q') | Some(3) => {
                    drop(term);
                    return Ok(());
                }
                Some(b'c') => {
                    opts.sort_mem = false;
                    true
                }
                Some(b'm') => {
                    opts.sort_mem = true;
                    true
                }
                Some(b'k') => {
                    opts.show_kthreads = !opts.show_kthreads;
                    true
                }
                Some(b'+') | Some(b'=') => {
                    opts.interval_s = (opts.interval_s + 0.5).min(60.0);
                    true
                }
                Some(b'-') => {
                    opts.interval_s = (opts.interval_s - 0.5).max(0.25);
                    true
                }
                _ => false,
            };
            if redraw {
                draw(&model, derived.as_ref(), &opts);
            }
        }
    }
}
