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

mod client;
mod disk;
mod forward;
mod net;
mod serve;

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
        #[arg(long, value_parser = parse_u64, default_value = "0x10000000")]
        ring_base: u64,
        /// Size of the ring region.
        #[arg(long, value_parser = parse_u64, default_value = "0x1000000")]
        ring_size: u64,
        /// Use the command line exactly as given (no memmap/phi.ring parameters).
        #[arg(long)]
        raw_cmdline: bool,
        /// Do not tail the console after sending the boot interrupt.
        #[arg(long)]
        no_console: bool,
        /// Also watch a 32-bit word in card memory (the kernel's knc_boot_mark)
        /// and print it as a POST-style code whenever it changes.
        #[arg(long, value_parser = parse_u64)]
        watch: Option<u64>,
        /// Write everything into card memory but do not send the boot
        /// interrupt (bisecting host resets); implies --no-console.
        #[arg(long)]
        load_only: bool,
        /// Bridge the ring network channel to a host TAP device of this name
        /// (created if absent) so the card is reachable over IP; needs root.
        #[arg(long)]
        net: Option<String>,
        /// IPv4 address with prefix length assigned to the TAP device.
        #[arg(long, default_value = "10.9.0.1/24")]
        net_addr: String,
        /// Forward 127.0.0.1:HOSTPORT to the card's CARDPORT through a userspace
        /// network stack on the ring (no root; "2222:22", or "2222" for port 22).
        #[arg(long, conflicts_with = "net")]
        forward: Option<String>,
        /// Also serve the local control socket (at PATH, else
        /// /run/phictl/control.sock as root, else phictl/control.sock in the user runtime directory
        /// otherwise) so that exec, put, get and status can drive the card.
        #[arg(long, num_args = 0..=1, default_missing_value = "auto")]
        serve: Option<PathBuf>,
        /// Serve this disk image to the card as /dev/phiblk0 through the ring's
        /// block channel (no root; the card mounts it on /data, see disk.md).
        #[arg(long)]
        disk: Option<PathBuf>,
        /// Serve the disk through aperture copies instead of the DMA engine.
        #[arg(long)]
        no_dma: bool,
        /// Uid allowed to use the control socket (default: SUDO_UID, else root).
        #[arg(long)]
        owner: Option<u32>,
    },
    /// Read a few bytes of card memory through the BAR0 aperture and print
    /// them. A single non-posted read, for testing the aperture in isolation.
    Peek {
        /// Card physical address (hex or decimal).
        #[arg(value_parser = parse_u64)]
        addr: u64,
        /// Number of bytes (at most 256).
        #[arg(long, default_value_t = 4)]
        len: usize,
    },
    /// Write one 8-byte value into card memory through the BAR0 aperture:
    /// a single posted write, for testing the aperture in isolation.
    Poke {
        /// Card physical address (hex or decimal).
        #[arg(value_parser = parse_u64)]
        addr: u64,
        /// 64-bit value (hex or decimal), written little-endian.
        #[arg(value_parser = parse_u64)]
        value: u64,
    },
    /// Write LEN zero bytes into card memory through the aperture, in chunks
    /// with a read-back after each (the loader's write path), and report the
    /// time. For finding the burst size that resets the host.
    Fill {
        /// Card physical address (hex or decimal).
        #[arg(value_parser = parse_u64)]
        addr: u64,
        /// Number of bytes.
        len: usize,
        /// Bytes per chunk between read-backs.
        #[arg(long, default_value_t = 4096)]
        chunk: usize,
        /// Do not read back after each chunk (unpaced posted writes).
        #[arg(long)]
        no_readback: bool,
    },
    /// Tail the card's console ring and forward stdin lines to it.
    Console {
        /// Card physical base of the ring region.
        #[arg(long, value_parser = parse_u64, default_value = "0x10000000")]
        ring_base: u64,
        /// Size of the ring region.
        #[arg(long, value_parser = parse_u64, default_value = "0x1000000")]
        ring_size: u64,
        /// Also watch a 32-bit word in card memory and print changes.
        #[arg(long, value_parser = parse_u64)]
        watch: Option<u64>,
        /// Bridge the ring network channel to a host TAP device of this name.
        #[arg(long)]
        net: Option<String>,
        /// IPv4 address with prefix length assigned to the TAP device.
        #[arg(long, default_value = "10.9.0.1/24")]
        net_addr: String,
        /// Forward 127.0.0.1:HOSTPORT to the card's CARDPORT through a userspace
        /// network stack on the ring (no root; "2222:22", or "2222" for port 22).
        #[arg(long, conflicts_with = "net")]
        forward: Option<String>,
    },
    /// Run a command on the card through the control socket; stdin, stdout,
    /// stderr and the exit status are relayed.
    Exec {
        /// Control socket of a running `phictl boot --serve` (default: PHICTL_SOCKET,
        /// else /run/phictl/control.sock if present, else the user runtime directory).
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Working directory on the card.
        #[arg(long)]
        cwd: Option<String>,
        /// Program and arguments, as on the card.
        #[arg(required = true, trailing_var_arg = true)]
        argv: Vec<String>,
    },
    /// Copy a local file to the card.
    Put {
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Local file.
        src: PathBuf,
        /// Destination path on the card.
        dst: String,
        /// File mode on the card (octal); default: the local file's mode.
        #[arg(long, value_parser = parse_mode)]
        mode: Option<u32>,
    },
    /// Copy a file from the card.
    Get {
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Path on the card.
        src: String,
        /// Local destination file.
        dst: PathBuf,
    },
    /// Check that the card agent answers on the control socket.
    Status {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

fn parse_mode(s: &str) -> std::result::Result<u32, String> {
    u32::from_str_radix(s, 8).map_err(|e| format!("octal mode expected: {e}"))
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
            load_only,
            watch,
            net,
            net_addr,
            forward,
            disk,
            no_dma,
            serve,
            owner,
        } => cmd_boot(
            cli.bdf.as_deref(),
            kernel,
            initrd,
            cmdline,
            ring_base,
            ring_size,
            raw_cmdline,
            !no_console && !load_only,
            load_only,
            watch,
            net.map(|n| (n, net_addr)),
            forward,
            disk,
            no_dma,
            serve.map(|p| {
                let p = if p.as_os_str() == "auto" { serve::default_socket(true) } else { p };
                (p, owner.unwrap_or_else(serve::default_owner))
            }),
        ),
        Cmd::Peek { addr, len } => {
            let card = open(cli.bdf.as_deref())?;
            card.wait_ready(std::time::Duration::from_secs(20))?;
            let mut buf = vec![0u8; len.min(256)];
            card.read_card_memory(addr, &mut buf)?;
            let hex: Vec<String> = buf.iter().map(|b| format!("{b:02x}")).collect();
            println!("{addr:#x}: {}", hex.join(" "));
            Ok(())
        }
        Cmd::Poke { addr, value } => {
            let card = open(cli.bdf.as_deref())?;
            card.wait_ready(std::time::Duration::from_secs(20))?;
            card.write_card_memory(addr, &value.to_le_bytes())?;
            println!("{addr:#x} <- {value:#x}");
            Ok(())
        }
        Cmd::Fill {
            addr,
            len,
            chunk,
            no_readback,
        } => {
            let card = open(cli.bdf.as_deref())?;
            card.wait_ready(std::time::Duration::from_secs(20))?;
            // An address-derived pattern, so the read-back check catches an
            // aperture that returns zeros or stale data.
            let data: Vec<u8> = (0..len).map(|i| ((addr as usize + i) as u8) ^ 0x5a).collect();
            let t0 = Instant::now();
            card.write_card_memory_paced(addr, &data, chunk, !no_readback)?;
            println!(
                "{len:#x} bytes at {addr:#x} in {:.3}s (chunk {chunk:#x}, readback {})",
                t0.elapsed().as_secs_f64(),
                !no_readback
            );
            Ok(())
        }
        Cmd::Console {
            ring_base,
            ring_size,
            watch,
            net,
            net_addr,
            forward,
        } => {
            let card = open(cli.bdf.as_deref())?;
            console(&card, ring_base, ring_size, watch, net.map(|n| (n, net_addr)), forward, None, false)
        }
        Cmd::Exec { socket, cwd, argv } => {
            let code = client::exec(&socket.unwrap_or_else(|| serve::default_socket(false)), cwd, argv)?;
            std::process::exit(code);
        }
        Cmd::Put { socket, src, dst, mode } => client::put(&socket.unwrap_or_else(|| serve::default_socket(false)), &src, dst, mode),
        Cmd::Get { socket, src, dst } => client::get(&socket.unwrap_or_else(|| serve::default_socket(false)), src, &dst),
        Cmd::Status { socket } => client::status(&socket.unwrap_or_else(|| serve::default_socket(false))),
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
    load_only: bool,
    watch: Option<u64>,
    net: Option<(String, String)>,
    forward: Option<String>,
    disk: Option<PathBuf>,
    no_dma: bool,
    serve: Option<(PathBuf, u32)>,
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
        load_only,
    };
    let t0 = Instant::now();
    let r = boot(&card, &img)?;
    if load_only {
        println!(
            "loaded in {:.2}s; boot interrupt NOT sent (--load-only)",
            t0.elapsed().as_secs_f64()
        );
    } else {
        println!("boot interrupt sent to APIC {} after {:.2}s", r.apic_id, t0.elapsed().as_secs_f64());
    }
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
        thread::scope(|s| {
            if let Some((socket, owner)) = &serve {
                let listener = serve::bind(socket, *owner)?;
                let card = &card;
                s.spawn(move || {
                    if let Err(e) = serve::run(card, ring_base, ring_size, listener, *owner) {
                        eprintln!("[phictl] serve: {e:#}");
                    }
                });
            }
            console(&card, ring_base, ring_size, watch, net, forward, disk, !no_dma)
        })
    } else {
        Ok(())
    }
}

/// Tail the console ring and, when asked, bridge the network channel to a
/// TAP device from a second thread; both run until Ctrl-C.
#[allow(clippy::too_many_arguments)]
fn console(
    card: &Card,
    ring_base: u64,
    ring_size: u64,
    watch: Option<u64>,
    net: Option<(String, String)>,
    forward: Option<String>,
    disk: Option<PathBuf>,
    dma: bool,
) -> Result<()> {
    thread::scope(|s| {
        if let Some(spec) = &forward {
            let (host_port, card_port) = forward::parse_ports(spec)?;
            s.spawn(move || {
                if let Err(e) = forward::run(card, ring_base, ring_size, host_port, card_port) {
                    eprintln!("[phictl] forward: {e:#}");
                }
            });
        }
        if let Some(path) = &disk {
            s.spawn(move || {
                if let Err(e) = disk::run(card, ring_base, ring_size, path, dma) {
                    eprintln!("[phictl] disk: {e:#}");
                }
            });
        }
        if let Some((name, addr)) = &net {
            s.spawn(move || {
                if let Err(e) = net::bridge(card, ring_base, ring_size, name, addr) {
                    eprintln!("[phictl] net: {e:#}");
                }
            });
        }
        console_loop(card, ring_base, ring_size, watch)
    })
}

/// Tail the console ring, print POST code changes, forward stdin lines.
fn console_loop(card: &Card, ring_base: u64, ring_size: u64, watch: Option<u64>) -> Result<()> {
    if ring_base + ring_size > memory::GDDR_BYTES_3120A {
        anyhow::bail!("ring region beyond card memory");
    }
    let mut mem = ApertureRegion::new(card.aperture(), ring_base as usize, ring_size as usize);
    // The ring header may not be there yet (or may never be, if the aperture
    // round trip is broken): tail the POST code regardless and keep retrying.
    let mut endpoints = Region::open(&mem)
        .ok()
        .and_then(|r| r.host_endpoints(&mem, ChannelKind::Console).ok());
    if endpoints.is_none() {
        eprintln!("[phictl] ring region at {ring_base:#x} has no valid header; tailing POST codes, retrying the ring every 500 ms");
    }
    let mut next_retry = Instant::now() + Duration::from_millis(500);

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
    let mut last_mark = 0u32;
    let start = Instant::now();
    eprintln!("[phictl] console: tailing c2h ring at {ring_base:#x}; Ctrl-C to stop");
    loop {
        if endpoints.is_none() && Instant::now() >= next_retry {
            endpoints = Region::open(&mem)
                .ok()
                .and_then(|r| r.host_endpoints(&mem, ChannelKind::Console).ok());
            if endpoints.is_some() {
                eprintln!("[phictl] +{:>8.3}s ring region header valid", start.elapsed().as_secs_f64());
            }
            next_retry = Instant::now() + Duration::from_millis(500);
        }
        let n = match &endpoints {
            Some((_, consumer)) => consumer.pop(&mut mem, &mut buf),
            None => 0,
        };
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
        if let Some(addr) = watch {
            let mut w = [0u8; 4];
            if card.read_card_memory(addr, &mut w).is_ok() {
                let mark = u32::from_le_bytes(w);
                if mark != last_mark {
                    let p = phi_regs::postcode::Postcode(mark);
                    eprintln!(
                        "[phictl] +{:>8.3}s mark \"{}\" {}",
                        start.elapsed().as_secs_f64(),
                        p.text(),
                        p.describe().unwrap_or("")
                    );
                    last_mark = mark;
                }
            }
        }
        let flags = Region::card_boot_flags(&mem);
        if flags != last_flags {
            eprintln!("[phictl] +{:>8.3}s card flags {flags:#x}", start.elapsed().as_secs_f64());
            last_flags = flags;
        }
        while let Ok(line) = rx.try_recv() {
            let Some((producer, _)) = &endpoints else { continue };
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
