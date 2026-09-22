//! phitop: a resource viewer for the cards in the spirit of glances, run on
//! the host. Every interval it asks each card's daemon (`phictl boot
//! --serve`) for a `Stat` sample, which the card's agent answers from /proc
//! and sysfs, and for the daemon's own PCIe byte counters (`Traffic`), then
//! draws the load of every hardware thread on a core grid, the die
//! temperatures, memory and swap, PCIe rates by path and direction, the
//! card's disk and network rates, and its processes.
//!
//! With one card named (`-c N`, `PHI_CARD`, `--socket` or `PHICTL_SOCKET`)
//! that card fills the screen. With none named, every card in
//! `phictl cards` gets its own block, sampled on its own socket and drawn
//! from its own model, so one card's load never colours another's; `n`
//! and `p` focus one card, `a` goes back to all. See main.md.

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
use view::{CardHeader, CardView, Options};

#[derive(Parser)]
#[command(about = "Resource viewer for the Xeon Phi cards, from the host")]
struct Args {
    /// Card index, 0 to 15 (default: $PHI_CARD; else every card, each in its own block).
    #[arg(short, long)]
    card: Option<usize>,
    /// Seconds between samples.
    #[arg(short, long, default_value_t = 1.0)]
    interval: f64,
    /// Print this many frames as plain text and exit (no terminal control).
    #[arg(short, long)]
    batch: Option<u32>,
    /// One daemon's control socket (default: PHICTL_SOCKET, else the card's).
    #[arg(long)]
    socket: Option<PathBuf>,
    /// Show the kernel threads the card reports (those that used CPU time).
    #[arg(short, long)]
    kthreads: bool,
}

/// One card on the screen: where to ask, and what it has said so far.
struct Slot {
    index: usize,
    name: String,
    bdf: String,
    socket: PathBuf,
    model: Model,
    derived: Option<Derived>,
    error: Option<String>,
}

impl Slot {
    fn new(index: usize, bdf: String, socket: PathBuf) -> Slot {
        Slot {
            index,
            name: phi_vfio::cards::hostname(index),
            bdf,
            socket,
            model: Model::default(),
            derived: None,
            error: None,
        }
    }

    fn poll(&mut self) {
        match sample(&self.socket) {
            Ok(s) => {
                self.error = None;
                if let Some(d) = self.model.update(s) {
                    self.derived = Some(d);
                }
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    fn view(&self) -> CardView<'_> {
        CardView {
            head: CardHeader {
                index: self.index,
                name: &self.name,
                bdf: &self.bdf,
            },
            data: match (self.model.current(), self.derived.as_ref()) {
                (Some(s), Some(d)) => Some((s, d)),
                _ => None,
            },
            error: self.error.as_deref(),
        }
    }
}

/// The socket of card `index` for a client: a root daemon's when one
/// answers there, else the card's socket in the user's runtime directory
/// (`phi_vfio::cards`).
fn socket_of(index: usize) -> PathBuf {
    // SAFETY: geteuid has no preconditions.
    let root = unsafe { libc::geteuid() == 0 };
    let root_path = phi_vfio::cards::root_socket_path(index);
    if root || UnixStream::connect(&root_path).is_ok() {
        return root_path;
    }
    phi_vfio::cards::socket_path(index)
}

/// Which cards to watch. One, when anything names one; else all of them.
fn slots(args: &Args) -> Result<Vec<Slot>> {
    let env_card = phi_vfio::cards::index_from_env()?;
    let one = args.card.or(env_card);
    if let Some(s) = &args.socket {
        let i = one.unwrap_or(0);
        return Ok(vec![Slot::new(i, String::new(), s.clone())]);
    }
    if let Ok(p) = std::env::var("PHICTL_SOCKET") {
        let i = one.unwrap_or(0);
        return Ok(vec![Slot::new(i, String::new(), PathBuf::from(p))]);
    }
    let cards = phi_vfio::cards::list()?;
    if let Some(i) = one {
        phi_vfio::cards::check_index(i)?;
        let bdf = cards.iter().find(|c| c.index == i).map(|c| c.bdf.clone()).unwrap_or_default();
        return Ok(vec![Slot::new(i, bdf, socket_of(i))]);
    }
    if cards.is_empty() {
        bail!("no Xeon Phi card is known to this host (phictl cards)");
    }
    Ok(cards.into_iter().map(|c| Slot::new(c.index, c.bdf, socket_of(c.index))).collect())
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

/// The frame for the current focus: one card in full, or every card in
/// its own block.
fn frame(slots: &[Slot], focus: Option<usize>, o: &mut Options, cols: usize, rows: usize) -> String {
    match focus {
        Some(i) => {
            let s = &slots[i];
            o.error = s.error.clone();
            match (s.model.current(), s.derived.as_ref()) {
                (Some(cur), Some(d)) => view::render(cur, d, o, cols, rows),
                (Some(_), None) => "\x1b[Hsampling...\x1b[K".into(),
                (None, _) => format!("\x1b[H{}\x1b[K", o.error.as_deref().unwrap_or("connecting...")),
            }
        }
        None => {
            o.error = None;
            let views: Vec<CardView> = slots.iter().map(Slot::view).collect();
            view::render_multi(&views, o, cols, rows)
        }
    }
}

fn draw(slots: &[Slot], focus: Option<usize>, o: &mut Options) {
    let (cols, rows) = term::Term::size();
    let f = frame(slots, focus, o, cols, rows);
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(f.as_bytes());
    let _ = out.flush();
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut slots = slots(&args)?;
    // One card fills the screen; several start as blocks.
    let mut focus: Option<usize> = if slots.len() == 1 { Some(0) } else { None };
    let mut opts = Options {
        sort_mem: false,
        show_kthreads: args.kthreads,
        color: false,
        interval_s: args.interval.clamp(0.2, 60.0),
        error: None,
    };
    let poll = |slots: &mut Vec<Slot>, focus: Option<usize>| match focus {
        Some(i) => slots[i].poll(),
        None => slots.iter_mut().for_each(Slot::poll),
    };
    if let Some(frames) = args.batch {
        let mut printed = 0;
        while printed < frames {
            poll(&mut slots, focus);
            let ready = match focus {
                Some(i) => slots[i].derived.is_some(),
                None => slots.iter().any(|s| s.derived.is_some()),
            };
            if ready {
                println!("{}", frame(&slots, focus, &mut opts, 132, 32));
                printed += 1;
            } else if let Some(e) = slots.iter().find_map(|s| s.error.as_ref()) {
                eprintln!("phitop: {e}");
            }
            std::thread::sleep(Duration::from_secs_f64(opts.interval_s));
        }
        return Ok(());
    }
    let term = term::Term::open().context("standard input is not a terminal (use --batch N for plain text)")?;
    opts.color = true;
    let mut next = Instant::now();
    loop {
        poll(&mut slots, focus);
        draw(&slots, focus, &mut opts);
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
            let n = slots.len();
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
                Some(b'n') if n > 1 => {
                    focus = Some(focus.map_or(0, |i| (i + 1) % n));
                    true
                }
                Some(b'p') if n > 1 => {
                    focus = Some(focus.map_or(n - 1, |i| (i + n - 1) % n));
                    true
                }
                Some(b'a') if n > 1 => {
                    focus = None;
                    true
                }
                _ => false,
            };
            if redraw {
                draw(&slots, focus, &mut opts);
            }
        }
    }
}
