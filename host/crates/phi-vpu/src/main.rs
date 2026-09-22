//! `phi-vpu`: hand AVX-512 work to the card's vector units and get the
//! answer back.
//!
//! The host writes its data into the shared window, fills in a request,
//! rings the doorbell, and waits for the card's reply. The card runs the
//! program's own AVX-512 arithmetic, translated to its instruction set
//! ahead of time, on as many vector units as asked.
//!
//! ```text
//! phi-vpu status                      is a worker polling?
//! phi-vpu poly --n 1048576 --threads 57 --repeat 5
//! ```
//!
//! `poly` checks every returned lane against this host's own fused
//! multiply-add hardware before it reports a speed, because a fast wrong
//! answer is worth nothing.

mod proto;

use std::ptr;
use std::sync::atomic::{fence, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};

use proto::*;

#[derive(Parser)]
#[command(about = "Drive the Xeon Phi's vector units as an AVX-512 co-processor", version)]
struct Cli {
    /// Card index, 0 to 15 (default: $PHI_CARD, else 0); picks that card's
    /// host-memory window, /dev/shm/phi-hostmem or phi-hostmem-N.
    #[arg(short, long, global = true)]
    card: Option<usize>,
    /// The shared window, as the host sees it (default: the card's).
    #[arg(long, global = true)]
    window: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

/// The window path the command line names: `--window`, else the card's.
fn window_path(cli: &Cli) -> Result<String> {
    if let Some(w) = &cli.window {
        return Ok(w.clone());
    }
    let index = match cli.card {
        Some(i) => {
            phi_vfio::cards::check_index(i)?;
            i
        }
        None => phi_vfio::cards::index_from_env()?.unwrap_or(0),
    };
    Ok(phi_vfio::cards::hostmem_path(index).display().to_string())
}

#[derive(Subcommand)]
enum Cmd {
    /// Is a card worker polling? Show the control words.
    Status,
    /// Evaluate a degree-30 polynomial on the card and check every lane
    /// against this host's FMA hardware.
    Poly {
        /// Elements; rounded down to a multiple of 128.
        #[arg(long, default_value_t = 65536)]
        n: u64,
        /// Card threads to spread the work across (1 to 228).
        #[arg(long, default_value_t = 57)]
        threads: u32,
        /// Submit the same request this many times and report each.
        #[arg(long, default_value_t = 1)]
        repeat: u32,
    },
}

/// The shared window, mapped.
struct Window {
    base: *mut u8,
    len: usize,
}

impl Window {
    fn open(path: &str, len: usize) -> Result<Window> {
        let cpath = std::ffi::CString::new(path)?;
        // SAFETY: plain open and mmap of a file the user named.
        let base = unsafe {
            let fd = libc::open(cpath.as_ptr(), libc::O_RDWR);
            if fd < 0 {
                return Err(std::io::Error::last_os_error()).with_context(|| format!("open {path}"));
            }
            let p = libc::mmap(ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
            libc::close(fd);
            if p == libc::MAP_FAILED {
                return Err(std::io::Error::last_os_error()).with_context(|| format!("mmap {len} bytes of {path}"));
            }
            p as *mut u8
        };
        Ok(Window { base, len })
    }

    /// Read a control word. Volatile, because the card writes here.
    fn read<T: Copy>(&self, off: usize) -> T {
        assert!(off + std::mem::size_of::<T>() <= self.len);
        // SAFETY: bounds checked; the window is mapped for the life of self.
        unsafe { ptr::read_volatile(self.base.add(off) as *const T) }
    }

    fn write<T: Copy>(&self, off: usize, v: T) {
        assert!(off + std::mem::size_of::<T>() <= self.len);
        // SAFETY: as above.
        unsafe { ptr::write_volatile(self.base.add(off) as *mut T, v) }
    }

    fn put(&self, off: u64, bytes: &[u8]) {
        let off = off as usize;
        assert!(off + bytes.len() <= self.len);
        // SAFETY: as above.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), self.base.add(off), bytes.len()) }
    }

    fn get(&self, off: u64, out: &mut [u8]) {
        let off = off as usize;
        assert!(off + out.len() <= self.len);
        // SAFETY: as above.
        unsafe { ptr::copy_nonoverlapping(self.base.add(off), out.as_mut_ptr(), out.len()) }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: unmapping what open() mapped.
        unsafe { libc::munmap(self.base as *mut libc::c_void, self.len) };
    }
}

/// Is the card's worker polling?
fn worker_ready(w: &Window) -> bool {
    w.read::<u64>(OFF_READY) == MAGIC
}

/// Wait for a worker that is alive now, not one that was alive once.
///
/// The readiness word stays in the window after a worker dies, so a
/// check that only reads it is satisfied by a corpse and the request
/// then waits its full timeout for an answer that never comes. The word
/// is cleared first; a live worker re-asserts it on every poll, within
/// a millisecond even when it is idle and sleeping between polls.
fn wait_ready(w: &Window, timeout: Duration) -> Result<()> {
    w.write(OFF_READY, 0u64);
    fence(Ordering::SeqCst);
    let give_up = Instant::now() + timeout;
    while !worker_ready(w) {
        if Instant::now() > give_up {
            bail!("no card worker is polling the window (scripts/phi-vpu.sh start)");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

/// Ring the doorbell and wait for the answer.
///
/// The request fields are written first, then a fence, then the sequence
/// number, which is the only word the card polls. The card writes its
/// reply fields and then the echoed sequence number last, so seeing the
/// number means the rest of the reply is there.
fn submit(w: &Window, req: Request, timeout: Duration) -> Result<(Reply, Duration)> {
    let seq = w.read::<u64>(OFF_REQ) + 1;
    let staged = Request { seq: seq - 1, ..req };
    w.write(OFF_REQ, staged);
    fence(Ordering::SeqCst);
    let t0 = Instant::now();
    w.write(OFF_REQ, seq);
    fence(Ordering::SeqCst);
    let give_up = t0 + timeout;
    loop {
        let rep: Reply = w.read(OFF_REPLY);
        if rep.seq == seq {
            return Ok((rep, t0.elapsed()));
        }
        if Instant::now() > give_up {
            bail!("the card did not answer request {seq} within {timeout:?}");
        }
    }
}

const DEG: usize = 30;
const LANES: usize = 16;

fn poly(w: &Window, n: u64, threads: u32, repeat: u32) -> Result<()> {
    let n = n - n % CHUNK;
    if n == 0 {
        bail!("n must be at least {CHUNK}");
    }
    wait_ready(w, Duration::from_secs(5))?;

    // Every region starts on a block and is followed by its own slack:
    // the card reads and writes whole blocks (see proto.rs).
    let in_off = OFF_DATA;
    let coef_off = round_up(in_off + n * 4);
    let out_off = round_up(coef_off + ((DEG + 1) * LANES * 4) as u64);

    // Coefficients, one copy per lane as the kernel loads them.
    let cv: Vec<f32> = (0..=DEG).map(|k| 1.0f32 + k as f32 * 0.01f32).collect();
    let mut coef = Vec::with_capacity((DEG + 1) * LANES);
    for &c in &cv {
        coef.extend(std::iter::repeat_n(c, LANES));
    }
    let input: Vec<f32> = (0..n).map(|i| 0.5f32 + (i % 64) as f32 * 0.001f32).collect();

    w.put(in_off, as_bytes(&input));
    w.put(coef_off, as_bytes(&coef));
    w.put(out_off, &vec![0xa5u8; n as usize * 4]);

    // What this host's own fused multiply-add unit says, computed once.
    let want: Vec<u32> = input
        .iter()
        .map(|&x| {
            let mut acc = cv[0];
            for &c in &cv[1..] {
                acc = x.mul_add(acc, c);
            }
            acc.to_bits()
        })
        .collect();

    let req = Request {
        seq: 0,
        kernel: K_POLY30,
        threads,
        n,
        in_off,
        out_off,
        aux_off: coef_off,
        aux_len: ((DEG + 1) * LANES * 4) as u64,
    };

    let flops = 2.0 * DEG as f64 * n as f64;
    let mut got = vec![0u8; n as usize * 4];
    let mut best_compute = u64::MAX;
    for _ in 0..repeat {
        w.put(out_off, &vec![0xa5u8; n as usize * 4]);
        let (rep, wall) = submit(w, req, Duration::from_secs(60))?;
        if rep.status != OK {
            bail!("card reported status {}: {}", rep.status, status_name(rep.status));
        }
        w.get(out_off, &mut got);
        let bad = got
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .zip(&want)
            .enumerate()
            .filter(|(_, (g, w))| g != *w)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        println!(
            "n={n:<9} threads={:<3} wall {:8.3} ms  pull {:7.3}  compute {:7.3}  push {:7.3}  total {:7.3} ms  {:8.1} GFLOP/s on the vector units",
            rep.threads,
            wall.as_secs_f64() * 1e3,
            rep.pull_ns as f64 / 1e6,
            rep.compute_ns as f64 / 1e6,
            rep.push_ns as f64 / 1e6,
            rep.total_ns as f64 / 1e6,
            flops / (rep.compute_ns as f64 / 1e9) / 1e9,
        );
        if let Some(&first) = bad.first() {
            return Err(anyhow!(
                "WRONG: {} of {n} lanes differ from this host's FMA3 hardware, first at {first}",
                bad.len()
            ));
        }
        best_compute = best_compute.min(rep.compute_ns);
    }
    println!("  all {n} lanes bit-identical to this host's FMA3 hardware, every run");
    if repeat > 1 {
        println!(
            "  best compute {:.3} ms = {:.1} GFLOP/s",
            best_compute as f64 / 1e6,
            flops / (best_compute as f64 / 1e9) / 1e9
        );
    }
    Ok(())
}

fn as_bytes(v: &[f32]) -> &[u8] {
    // SAFETY: f32 has no padding and any bit pattern is a valid byte.
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

fn status(w: &Window) -> Result<()> {
    let req: Request = w.read(OFF_REQ);
    let rep: Reply = w.read(OFF_REPLY);
    println!(
        "worker: {}",
        if worker_ready(w) {
            "polling"
        } else {
            "not polling (no readiness word)"
        }
    );
    println!("request: seq={} kernel={} n={} threads={}", req.seq, req.kernel, req.n, req.threads);
    println!(
        "reply:   seq={} status={} ({}) compute={:.3} ms total={:.3} ms threads={}",
        rep.seq,
        rep.status,
        status_name(rep.status),
        rep.compute_ns as f64 / 1e6,
        rep.total_ns as f64 / 1e6,
        rep.threads
    );
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let window = window_path(&cli)?;
    match cli.cmd {
        Cmd::Status => {
            let w = Window::open(&window, OFF_DATA as usize)?;
            status(&w)
        }
        Cmd::Poly { n, threads, repeat } => {
            let n = n - n % CHUNK;
            let len = round_up(OFF_DATA + 2 * round_up(n * 4) + BLOCK + ((DEG + 1) * LANES * 4) as u64) + BLOCK;
            let w = Window::open(&window, len as usize)?;
            poly(&w, n, threads, repeat)
        }
    }
}
