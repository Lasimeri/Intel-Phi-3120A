//! `phictl`: talk to the card.
//!
//! Every subcommand opens the card through VFIO (`scripts/bind-vfio.sh`
//! must have run) unless it only needs sysfs. Device selection: `--bdf`,
//! then `PHI_BDF`, then the first 8086:225d device found.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use phi_hw::boot::{boot, BootImage};
use phi_hw::ringmem::ApertureRegion;
use phi_hw::Card;
use phi_regs::memory;
use phi_regs::sbox;
use phi_ring::{ChannelKind, Region};

/// Control the Intel Xeon Phi 3120A.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// PCI address, e.g. 0000:2e:00.0 (default: $PHI_BDF or autodetect).
    #[arg(long, global = true)]
    bdf: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Show PCI identity, POST code, scratchpads, and bootstrap state.
    Info,
    /// Print the POST code; with --watch, print every change.
    Postcode {
        /// Keep printing changes until Ctrl-C or --timeout.
        #[arg(long)]
        watch: bool,
        /// Stop watching after this many seconds.
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Read one scratchpad (0..15) or all of them.
    Spad {
        /// Index.
        n: Option<u32>,
    },
    /// Dump the named SBOX registers.
    Regs,
    /// Reset the card to its bootstrap and wait for it to report ready.
    Reset {
        /// Seconds to wait for the ready flag.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Load a kernel (and optional initramfs) into card memory and start it.
    Boot {
        /// bzImage path.
        #[arg(long)]
        kernel: PathBuf,
        /// Initramfs (cpio, optionally compressed) path.
        #[arg(long)]
        initrd: Option<PathBuf>,
        /// Kernel command line.
        #[arg(long, default_value = "earlyprintk=phiring,keep loglevel=8")]
        cmdline: String,
        /// Card physical base of the ring region (hex or decimal).
        #[arg(long, value_parser = parse_u64, default_value = "0x2000000")]
        ring_base: u64,
        /// Size of the ring region.
        #[arg(long, value_parser = parse_u64, default_value = "0x100000")]
        ring_size: u64,
        /// Use the command line exactly as given (no memmap/phi.ring parameters).
        #[arg(long)]
        raw_cmdline: bool,
        /// Do not tail the console after sending the boot interrupt.
        #[arg(long)]
        no_console: bool,
    },
    /// Tail the card's console ring and forward stdin lines to it.
    Console {
        /// Card physical base of the ring region.
        #[arg(long, value_parser = parse_u64, default_value = "0x2000000")]
        ring_base: u64,
        /// Size of the ring region.
        #[arg(long, value_parser = parse_u64, default_value = "0x100000")]
        ring_size: u64,
    },
}

fn parse_u64(s: &str) -> std::result::Result<u64, String> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).map_err(|e| e.to_string())
    } else {
        s.parse().map_err(|e: std::num::ParseIntError| e.to_string())
    }
}

fn open(bdf: Option<&str>) -> Result<Card> {
    let bdf = phi_vfio::sysfs::resolve_bdf(bdf)?;
    if phi_vfio::sysfs::driver_of(&bdf).as_deref() != Some("vfio-pci") {
        anyhow::bail!("{bdf} is not bound to vfio-pci; run: sudo scripts/bind-vfio.sh");
    }
    Card::open(&bdf).with_context(|| format!("opening {bdf}"))
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info => cmd_info(cli.bdf.as_deref()),
        Cmd::Postcode { watch, timeout } => cmd_postcode(cli.bdf.as_deref(), watch, timeout),
        Cmd::Spad { n } => cmd_spad(cli.bdf.as_deref(), n),
        Cmd::Regs => cmd_regs(cli.bdf.as_deref()),
        Cmd::Reset { timeout } => cmd_reset(cli.bdf.as_deref(), timeout),
        Cmd::Boot {
            kernel,
            initrd,
            cmdline,
            ring_base,
            ring_size,
            raw_cmdline,
            no_console,
        } => cmd_boot(
            cli.bdf.as_deref(),
            kernel,
            initrd,
            cmdline,
            ring_base,
            ring_size,
            raw_cmdline,
            !no_console,
        ),
        Cmd::Console { ring_base, ring_size } => {
            let card = open(cli.bdf.as_deref())?;
            console(&card, ring_base, ring_size)
        }
    }
}

fn cmd_info(bdf: Option<&str>) -> Result<()> {
    let card = open(bdf)?;
    let s = card.status()?;
    println!("device        {}", s.bdf);
    println!(
        "ids           {:04x}:{:04x} subsystem {:04x}:{:04x}",
        s.ids.0, s.ids.1, s.ids.2, s.ids.3
    );
    println!(
        "pci command   {:#06x} (memory {} bus master {})",
        s.command,
        s.command & 2 != 0,
        s.command & 4 != 0
    );
    println!("aperture      {} MiB mapped", s.aperture_len >> 20);
    println!(
        "postcode      {:#010x} -> \"{}\" {}",
        s.postcode.0,
        s.postcode.text(),
        s.postcode.describe().unwrap_or("(unknown code)")
    );
    println!(
        "spad2         {:#010x}: ready={} apic_id={} download_addr={:#x}",
        s.download.0,
        s.download.ready(),
        s.download.apic_id(),
        s.download.download_addr()
    );
    println!(
        "spad4         {:#010x}: threads/core {:#x} l2 {} KiB channels {} icc_div {} ref {} MHz soft_reset={}",
        s.platform.0,
        s.platform.thread_mask(),
        s.platform.l2_kib(),
        s.platform.memory_channels(),
        s.platform.icc_divider(),
        s.platform.reference_mhz(),
        s.platform.soft_reset()
    );
    println!(
        "clk ratio     {:#010x}: fb {} ff {} -> core {} MHz",
        s.clock_ratio.0,
        s.clock_ratio.feedback(),
        s.clock_ratio.feedforward_div(),
        s.clock_ratio.core_mhz(s.platform)
    );
    for (i, v) in s.spads.iter().enumerate() {
        println!("spad{i:<2}        {v:#010x}");
    }
    Ok(())
}

fn cmd_postcode(bdf: Option<&str>, watch: bool, timeout: Option<u64>) -> Result<()> {
    let card = open(bdf)?;
    let mut last = card.postcode();
    println!("\"{}\" {}", last.text(), last.describe().unwrap_or(""));
    if !watch {
        return Ok(());
    }
    let start = Instant::now();
    loop {
        let now = card.postcode();
        if now != last {
            println!(
                "+{:>8.3}s \"{}\" {}",
                start.elapsed().as_secs_f64(),
                now.text(),
                now.describe().unwrap_or("")
            );
            last = now;
        }
        if let Some(t) = timeout {
            if start.elapsed() >= Duration::from_secs(t) {
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn cmd_spad(bdf: Option<&str>, n: Option<u32>) -> Result<()> {
    let card = open(bdf)?;
    match n {
        Some(n) if n < sbox::SPAD_COUNT => println!("spad{n} {:#010x}", card.spad(n)),
        Some(n) => anyhow::bail!("scratchpad index {n} out of range 0..{}", sbox::SPAD_COUNT),
        None => {
            for i in 0..sbox::SPAD_COUNT {
                println!("spad{i:<2} {:#010x}", card.spad(i));
            }
        }
    }
    Ok(())
}

fn cmd_regs(bdf: Option<&str>) -> Result<()> {
    let card = open(bdf)?;
    let named: [(&str, u32); 10] = [
        ("RGCR", sbox::RGCR),
        ("SICR0", sbox::SICR0),
        ("SICE0", sbox::SICE0),
        ("SICC0", sbox::SICC0),
        ("SIAC0", sbox::SIAC0),
        ("MXAR0", sbox::MXAR0),
        ("MSIXPBACR", sbox::MSIXPBACR),
        ("APICICR7.lo", sbox::APICICR7),
        ("APICICR7.hi", sbox::APICICR7 + 4),
        ("SDBIC0", sbox::SDBIC0),
    ];
    println!("{:<14} {:>8}  value", "register", "offset");
    println!("{:<14} {:>#8x}  {:#010x}", "POSTCODE", sbox::POSTCODE, card.postcode().0);
    for (name, off) in named {
        println!("{:<14} {:>#8x}  {:#010x}", name, off, card.sbox_read(off));
    }
    for i in 0..sbox::SMPT_COUNT {
        let v = card.sbox_read(sbox::smpt(i));
        if v != 0 {
            println!("{:<14} {:>#8x}  {:#010x}", format!("SMPT{i:02}"), sbox::smpt(i), v);
        }
    }
    for i in 0..sbox::RDMASR_COUNT {
        println!(
            "{:<14} {:>#8x}  {:#010x}",
            format!("RDMASR{i}"),
            sbox::rdmasr(i),
            card.sbox_read(sbox::rdmasr(i))
        );
    }
    Ok(())
}

fn cmd_reset(bdf: Option<&str>, timeout: u64) -> Result<()> {
    let card = open(bdf)?;
    let r = card.reset(Duration::from_secs(timeout))?;
    println!(
        "spad2 before {:#010x}, after {:#010x}; ready after {:.2}s",
        r.spad2_before,
        r.spad2_after,
        r.took.as_secs_f64()
    );
    println!("POST code trace ({} distinct values):", r.trace.len());
    for (t, p) in &r.trace {
        println!(
            "  +{:>8.3}s {:#010x} \"{}\" {}",
            t.as_secs_f64(),
            p.0,
            p.text(),
            p.describe().unwrap_or("")
        );
    }
    let d = card.download_info();
    println!(
        "now: ready={} apic_id={} download_addr={:#x}",
        d.ready(),
        d.apic_id(),
        d.download_addr()
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_boot(
    bdf: Option<&str>,
    kernel: PathBuf,
    initrd: Option<PathBuf>,
    cmdline: String,
    ring_base: u64,
    ring_size: u64,
    raw_cmdline: bool,
    follow: bool,
) -> Result<()> {
    let card = open(bdf)?;
    let kernel_bytes = std::fs::read(&kernel).with_context(|| format!("reading {}", kernel.display()))?;
    let initrd_bytes = match &initrd {
        Some(p) => Some(std::fs::read(p).with_context(|| format!("reading {}", p.display()))?),
        None => None,
    };
    let img = BootImage {
        kernel: kernel_bytes,
        initrd: initrd_bytes,
        cmdline,
        ring_base,
        ring_size,
        raw_cmdline,
    };
    let t0 = Instant::now();
    let r = boot(&card, &img)?;
    println!("boot interrupt sent to APIC {} after {:.2}s", r.apic_id, t0.elapsed().as_secs_f64());
    println!(
        "  image     protocol {:#06x}, {} bytes at {:#x}",
        r.image.protocol, r.kernel_len, r.bootaddr
    );
    println!("  cmdline   at {:#x}: {}", r.cmdline_addr, r.cmdline);
    match r.initrd {
        Some((a, l)) => println!("  initramfs {l} bytes at {a:#x}"),
        None => println!("  initramfs none"),
    }
    println!("  ring      {ring_size:#x} bytes at {ring_base:#x}");
    if follow {
        console(&card, ring_base, ring_size)
    } else {
        Ok(())
    }
}

/// Tail the console ring, print POST code changes, forward stdin lines.
fn console(card: &Card, ring_base: u64, ring_size: u64) -> Result<()> {
    if ring_base + ring_size > memory::GDDR_BYTES_3120A {
        anyhow::bail!("ring region beyond card memory");
    }
    let mut mem = ApertureRegion::new(card.aperture(), ring_base as usize, ring_size as usize);
    let region = Region::open(&mem).context("ring region not formatted at that address (boot first, or check --ring-base)")?;
    let (producer, consumer) = region.host_endpoints(&mem, ChannelKind::Console)?;

    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            if tx.send(line + "\n").is_err() {
                break;
            }
        }
    });

    let mut out = io::stdout().lock();
    let mut buf = [0u8; 4096];
    let mut last_post = card.postcode();
    let mut last_flags = 0u64;
    let start = Instant::now();
    eprintln!("[phictl] console: tailing c2h ring at {ring_base:#x}; Ctrl-C to stop");
    loop {
        let n = consumer.pop(&mut mem, &mut buf);
        if n > 0 {
            out.write_all(&buf[..n])?;
            out.flush()?;
        }
        let post = card.postcode();
        if post != last_post {
            eprintln!(
                "[phictl] +{:>8.3}s postcode \"{}\" {}",
                start.elapsed().as_secs_f64(),
                post.text(),
                post.describe().unwrap_or("")
            );
            last_post = post;
        }
        let flags = Region::card_boot_flags(&mem);
        if flags != last_flags {
            eprintln!("[phictl] +{:>8.3}s card flags {flags:#x}", start.elapsed().as_secs_f64());
            last_flags = flags;
        }
        while let Ok(line) = rx.try_recv() {
            let mut bytes = line.as_bytes();
            while !bytes.is_empty() {
                let w = producer.push(&mut mem, bytes);
                bytes = &bytes[w..];
                if w == 0 {
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
        if n == 0 {
            thread::sleep(Duration::from_millis(1));
        }
    }
}
