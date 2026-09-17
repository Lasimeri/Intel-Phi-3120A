//! The card's state for `phitop`: one `Stat` sample from /proc and sysfs.
//! Counters are cumulative; the host differences consecutive samples.
//! The parsers take the file text so they can be tested on the host with
//! lines copied from the card. See stat.md.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use phi_rpc::{CpuStat, DevStat, MemStat, ProcStat, Stat};

/// PF_KTHREAD in the flags field of /proc/[pid]/stat (include/linux/sched.h).
const PF_KTHREAD: u64 = 0x0020_0000;
/// hwmon temperature channels the driver registers (kernel patch 0027).
const TEMP_CHANNELS: usize = 15;
/// Die temperature channels, the ones with a temp*_max attribute.
const DIE_CHANNELS: usize = 9;
/// Kernel threads are re-read every this many samples; between those a
/// known kernel thread is counted but not opened.
const KTHREAD_EVERY: u32 = 4;
/// Most processes sent in one sample. A `StatReply` must stay under
/// `phi_rpc::MAX_FRAME` (1 MiB): a process costs 28 bytes plus its name
/// (at most 15 bytes, TASK_COMM_LEN in include/linux/sched.h), the rest of
/// the sample about 3 KiB: 707 KB in all (the test below), a quarter spare.
const MAX_PROCS: usize = 16384;

/// What persists between samples.
#[derive(Default)]
pub struct Sampler {
    /// core_id of each CPU, read once from sysfs.
    cores: Vec<u16>,
    /// The "knc" hwmon directory, found once.
    hwmon: Option<PathBuf>,
    /// Kernel threads' CPU ticks when last read.
    kthread_ticks: HashMap<u32, u64>,
    page_kb: u64,
    samples: u32,
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn num<T: std::str::FromStr>(s: &str) -> Option<T> {
    s.trim().parse().ok()
}

/// "1.33" as hundredths; "139.78" as 13978. Extra decimals are dropped.
fn hundredths(s: &str) -> u64 {
    let mut it = s.trim().splitn(2, '.');
    let whole: u64 = it.next().and_then(num).unwrap_or(0);
    let frac = it.next().unwrap_or("");
    let frac: u64 = match frac.len() {
        0 => 0,
        1 => num::<u64>(frac).unwrap_or(0) * 10,
        _ => frac.get(..2).and_then(num).unwrap_or(0),
    };
    whole * 100 + frac
}

impl Sampler {
    /// Take one sample.
    pub fn sample(&mut self) -> Stat {
        if self.page_kb == 0 {
            // SAFETY: sysconf has no preconditions.
            let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            self.page_kb = if page > 0 { page as u64 / 1024 } else { 4 };
        }
        let mut s = Stat {
            uptime_ms: hundredths(
                read("/proc/uptime")
                    .split_whitespace()
                    .next()
                    .unwrap_or("0"),
            ) * 10,
            ..Stat::default()
        };
        s.cpus = self.cpus();
        s.mem = parse_meminfo(&read("/proc/meminfo"));
        parse_loadavg(&read("/proc/loadavg"), &mut s);
        self.sensors(&mut s);
        s.disks = parse_diskstats(&read("/proc/diskstats"));
        s.nets = parse_netdev(&read("/proc/net/dev"));
        self.procs(&mut s);
        s
    }

    fn cpus(&mut self) -> Vec<CpuStat> {
        let mut cpus = parse_cpus(&read("/proc/stat"));
        if self.cores.len() < cpus.len() {
            self.cores = (0..cpus.len())
                .map(|n| {
                    num(&read(format!(
                        "/sys/devices/system/cpu/cpu{n}/topology/core_id"
                    )))
                    .unwrap_or(0)
                })
                .collect();
        }
        for (c, core) in cpus.iter_mut().zip(&self.cores) {
            c.core = *core;
        }
        cpus
    }

    fn sensors(&mut self, s: &mut Stat) {
        if self.hwmon.is_none() {
            if let Ok(dir) = fs::read_dir("/sys/class/hwmon") {
                for e in dir.flatten() {
                    if read(e.path().join("name")).trim() == "knc" {
                        self.hwmon = Some(e.path());
                        break;
                    }
                }
            }
        }
        let Some(h) = &self.hwmon else { return };
        let millideg = |name: String| -> i16 {
            num::<i64>(&read(h.join(name))).map_or(-1, |v| (v / 1000) as i16)
        };
        s.temps = (1..=TEMP_CHANNELS)
            .map(|i| millideg(format!("temp{i}_input")))
            .collect();
        s.temp_peak = (1..=DIE_CHANNELS)
            .map(|i| millideg(format!("temp{i}_max")))
            .max()
            .unwrap_or(-1);
        s.vcore_mv = num(&read(h.join("in0_input"))).unwrap_or(0);
        s.core_mhz = num(&read(h.join("core_mhz"))).unwrap_or(0);
    }

    fn procs(&mut self, s: &mut Stat) {
        self.samples = self.samples.wrapping_add(1);
        let refresh = self.samples % KTHREAD_EVERY == 1;
        let mut seen: HashMap<u32, u64> = HashMap::with_capacity(self.kthread_ticks.len() + 16);
        let Ok(dir) = fs::read_dir("/proc") else {
            return;
        };
        for e in dir.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(num::<u32>) else {
                continue;
            };
            if !refresh {
                // Reading 1500 stat files a second costs a sixth of a hardware
                // thread; known kernel threads are re-read every fourth sample.
                if let Some(t) = self.kthread_ticks.get(&pid) {
                    s.nprocs += 1;
                    s.nkthreads += 1;
                    seen.insert(pid, *t);
                    continue;
                }
            }
            let Some(mut p) = parse_stat(pid, &read(e.path().join("stat")), self.page_kb) else {
                continue;
            };
            s.nprocs += 1;
            if !p.kthread {
                // The stat line's rss is the mm counter's global part, and with
                // 228 CPUs the per-CPU batch is 456 pages, so a small process
                // reads 0 there (measured 2026-09-17); status sums the parts.
                p.rss_kb = vm_rss_kb(&read(e.path().join("status"))).unwrap_or(p.rss_kb);
            }
            if p.kthread {
                s.nkthreads += 1;
                let moved = self.kthread_ticks.get(&pid) != Some(&p.ticks);
                seen.insert(pid, p.ticks);
                if !moved {
                    continue;
                }
            }
            if s.procs.len() < MAX_PROCS {
                s.procs.push(p);
            }
        }
        self.kthread_ticks = seen;
    }
}

/// The cpuN lines of /proc/stat, indexed by N (a gap leaves zeros); the
/// core id is filled in by the caller.
fn parse_cpus(text: &str) -> Vec<CpuStat> {
    let mut cpus: Vec<CpuStat> = Vec::with_capacity(256);
    for line in text.lines() {
        let mut f = line.split_whitespace();
        let Some(name) = f.next() else { continue };
        let Some(n) = name.strip_prefix("cpu").and_then(num::<usize>) else {
            continue;
        };
        let v: Vec<u64> = f.map(|x| num(x).unwrap_or(0)).collect();
        let at = |i: usize| v.get(i).copied().unwrap_or(0);
        // user nice system idle iowait irq softirq steal (fs/proc/stat.c)
        let busy = at(0) + at(1) + at(2) + at(5) + at(6) + at(7);
        let idle = at(3) + at(4);
        if cpus.len() <= n {
            cpus.resize(n + 1, CpuStat::default());
        }
        cpus[n] = CpuStat {
            core: 0,
            busy: busy as u32,
            idle: idle as u32,
        };
    }
    cpus
}

/// One /proc/[pid]/stat line. The command name is in parentheses and may
/// contain spaces and parentheses, so the fields are counted from the last
/// ')' (proc_pid_stat(5)).
fn parse_stat(pid: u32, line: &str, page_kb: u64) -> Option<ProcStat> {
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    let comm = line.get(open + 1..close)?.to_string();
    let rest: Vec<&str> = line.get(close + 1..)?.split_whitespace().collect();
    // After comm: state ppid pgrp session tty tpgid flags minflt cminflt
    // majflt cmajflt utime stime cutime cstime priority nice num_threads
    // itrealvalue starttime vsize rss.
    let at = |i: usize| -> u64 { rest.get(i).and_then(|x| num(x)).unwrap_or(0) };
    Some(ProcStat {
        pid,
        state: rest.first().and_then(|x| x.bytes().next()).unwrap_or(b'?'),
        kthread: at(6) & PF_KTHREAD != 0,
        ticks: at(11) + at(12),
        rss_kb: at(21) * page_kb,
        threads: at(17) as u32,
        comm,
    })
}

/// VmRSS from /proc/[pid]/status, in kB.
fn vm_rss_kb(status: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next())
        .and_then(num)
}

fn parse_meminfo(text: &str) -> MemStat {
    let mut m = MemStat::default();
    for line in text.lines() {
        let mut f = line.split_whitespace();
        let (Some(k), Some(v)) = (f.next(), f.next()) else {
            continue;
        };
        let v: u64 = num(v).unwrap_or(0);
        match k {
            "MemTotal:" => m.total_kb = v,
            "MemAvailable:" => m.avail_kb = v,
            "Buffers:" => m.buffers_kb = v,
            "Cached:" => m.cached_kb = v,
            "SwapTotal:" => m.swap_total_kb = v,
            "SwapFree:" => m.swap_free_kb = v,
            _ => {}
        }
    }
    m
}

/// /proc/loadavg: three averages (hundredths, capped by the wire's u16 at
/// 655.35) and "running/total".
fn parse_loadavg(text: &str, s: &mut Stat) {
    let f: Vec<&str> = text.split_whitespace().collect();
    for (i, l) in s.load.iter_mut().enumerate() {
        *l = f
            .get(i)
            .map_or(0, |x| hundredths(x).min(u16::MAX as u64) as u16);
    }
    if let Some((r, t)) = f.get(3).and_then(|x| x.split_once('/')) {
        s.running = num(r).unwrap_or(0);
        s.tasks = num(t).unwrap_or(0);
    }
}

/// Block devices served over the ring (phiblk*), bytes read and written:
/// /proc/diskstats fields 6 and 10 (sectors of 512 bytes,
/// Documentation/admin-guide/iostats.rst).
fn parse_diskstats(text: &str) -> Vec<DevStat> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let name = f.get(2)?;
            if !name.starts_with("phiblk") {
                return None;
            }
            Some(DevStat {
                name: name.to_string(),
                read: num::<u64>(f.get(5)?)? * 512,
                written: num::<u64>(f.get(9)?)? * 512,
            })
        })
        .collect()
}

/// Network interfaces except the loopback, bytes received and sent: the
/// first and ninth counters after "name:" in /proc/net/dev.
fn parse_netdev(text: &str) -> Vec<DevStat> {
    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let name = name.trim();
            if name == "lo" {
                return None;
            }
            let f: Vec<&str> = rest.split_whitespace().collect();
            Some(DevStat {
                name: name.to_string(),
                read: num(f.first()?)?,
                written: num(f.get(8)?)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_stat_line() {
        let p = parse_stat(1, "1 (init) S 0 0 0 0 -1 4194304 239 1184 0 0 0 103 0 42 20 0 1 0 57 1736704 139 18446744073709551615", 4).unwrap();
        assert_eq!(p.comm, "init");
        assert_eq!(p.state, b'S');
        assert!(!p.kthread);
        assert_eq!(p.ticks, 103);
        assert_eq!(p.threads, 1);
        assert_eq!(p.rss_kb, 556);
        let k = parse_stat(
            12,
            "12 (ksoftirqd/1) S 2 0 0 0 -1 69238848 0 0 0 0 0 3 0 0 20 0 1 0 2 0 0 0",
            4,
        )
        .unwrap();
        assert!(k.kthread);
        assert_eq!(k.ticks, 3);
        let odd = parse_stat(
            7,
            "7 (a (b) c) R 1 1 1 0 -1 4194560 0 0 0 0 5 6 0 0 20 0 3 0 9 0 250 0",
            4,
        )
        .unwrap();
        assert_eq!(odd.comm, "a (b) c");
        assert_eq!(odd.ticks, 11);
        assert_eq!(odd.threads, 3);
        assert_eq!(odd.rss_kb, 1000);
        // A process that vanished between readdir and read: empty text.
        assert_eq!(parse_stat(9, "", 4), None);
        // A line cut short still parses, with zeros for what is missing.
        let short = parse_stat(8, "8 (x) Z", 4).unwrap();
        assert_eq!(short.state, b'Z');
        assert_eq!(short.ticks, 0);
    }

    #[test]
    fn rss_from_status() {
        assert_eq!(
            vm_rss_kb("Name:\tinit\nVmPeak:\t 1696 kB\nVmRSS:\t     556 kB\nThreads:\t1\n"),
            Some(556)
        );
        assert_eq!(vm_rss_kb("Name:\tkthreadd\nThreads:\t1\n"), None);
    }

    #[test]
    fn hundredths_of_decimals() {
        assert_eq!(hundredths("1.33"), 133);
        assert_eq!(hundredths("0.5"), 50);
        assert_eq!(hundredths("139.78"), 13978);
        assert_eq!(hundredths("7"), 700);
        assert_eq!(hundredths("26811.34"), 2_681_134);
        assert_eq!(hundredths("1.234"), 123);
        assert_eq!(hundredths("x.\u{e9}"), 0);
        assert_eq!(hundredths(""), 0);
    }

    // Text copied from the card on 2026-09-17 (`phictl exec -- cat ...`).
    const PROC_STAT: &str = "cpu  392024 0 4204 611223798 210 0 31 0 0 0\n\
cpu0 1731 0 117 2679217 1 0 14 0 0 0\n\
cpu1 1730 0 117 2679219 0 0 6 0 0 0\n\
cpu3 5 4 3 100 20 1 1 1 9 9\n\
intr 12345 0 0\n\
ctxt 4711\n\
btime 1789000000\n\
processes 2131\n\
procs_running 1\n\
procs_blocked 0\n";

    #[test]
    fn cpus_from_proc_stat() {
        let c = parse_cpus(PROC_STAT);
        // The summary line is skipped; the missing cpu2 is a zero entry.
        assert_eq!(c.len(), 4);
        assert_eq!(c[0].busy, 1731 + 117 + 14);
        assert_eq!(c[0].idle, 2679217 + 1);
        assert_eq!(c[1].busy, 1730 + 117 + 6);
        assert_eq!(c[2], CpuStat::default());
        // user nice system irq softirq steal busy; idle and iowait idle.
        assert_eq!(c[3].busy, 5 + 4 + 3 + 1 + 1 + 1);
        assert_eq!(c[3].idle, 100 + 20);
        assert!(parse_cpus("").is_empty());
    }

    #[test]
    fn meminfo_fields() {
        let m = parse_meminfo(
            "MemTotal:        5805088 kB\nMemFree:         5398476 kB\nMemAvailable:    5379572 kB\n\
             Buffers:            8916 kB\nCached:            18912 kB\nSwapCached:            0 kB\n\
             SwapTotal:       4194300 kB\nSwapFree:        4194300 kB\n",
        );
        assert_eq!(
            m,
            MemStat {
                total_kb: 5_805_088,
                avail_kb: 5_379_572,
                buffers_kb: 8916,
                cached_kb: 18912,
                swap_total_kb: 4_194_300,
                swap_free_kb: 4_194_300,
            }
        );
    }

    #[test]
    fn loadavg_fields_and_cap() {
        let mut s = Stat::default();
        parse_loadavg("2.00 1.50 0.07 3/1529 2131\n", &mut s);
        assert_eq!(s.load, [200, 150, 7]);
        assert_eq!((s.running, s.tasks), (3, 1529));
        // The u16 caps a runaway load at 655.35.
        parse_loadavg("1000.00 655.35 655.36 1/1", &mut s);
        assert_eq!(s.load, [u16::MAX, u16::MAX, u16::MAX]);
        assert_eq!((s.running, s.tasks), (1, 1));
        parse_loadavg("", &mut s);
        assert_eq!(s.load, [0, 0, 0]);
    }

    #[test]
    fn diskstats_phiblk_only() {
        let d = parse_diskstats(
            "   7       0 loop0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n \
             251       0 phiblk0 7328 140 3672642 17089 3659 103 3671304 266084 0 7624 283303 0 0 0 0 16 129\n \
             251      16 phiblk1 3 0 24 5 2 0 8 1 0 5 6 0 0 0 0 1 0\n \
             251      32 phiblk2 short\n",
        );
        assert_eq!(
            d,
            vec![
                DevStat {
                    name: "phiblk0".into(),
                    read: 3_672_642 * 512,
                    written: 3_671_304 * 512,
                },
                DevStat {
                    name: "phiblk1".into(),
                    read: 24 * 512,
                    written: 8 * 512,
                },
            ]
        );
    }

    #[test]
    fn netdev_without_loopback() {
        let n = parse_netdev(
            "Inter-|   Receive                                                |  Transmit\n \
             face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    \
             lo:       0       0    0    0    0     0          0         0        0       0    0    0    0     0       0          0\n  \
             phi0:    7150      35    0    0    0     0          0         0     7151      67    0    0    0     0       0          0\n",
        );
        assert_eq!(
            n,
            vec![DevStat {
                name: "phi0".into(),
                read: 7150,
                written: 7151,
            }]
        );
    }

    #[test]
    fn largest_sample_fits_a_frame() {
        // MAX_PROCS processes with the longest names, 228 CPUs, 15 sensors
        // and a few devices encode under MAX_FRAME (see MAX_PROCS).
        let s = Stat {
            cpus: vec![CpuStat::default(); 228],
            temps: vec![-1; TEMP_CHANNELS],
            disks: vec![
                DevStat {
                    name: "phiblk0".into(),
                    ..DevStat::default()
                };
                8
            ],
            nets: vec![
                DevStat {
                    name: "phi0".into(),
                    ..DevStat::default()
                };
                8
            ],
            procs: vec![
                ProcStat {
                    comm: "abcdefghijklmno".into(),
                    ..ProcStat::default()
                };
                MAX_PROCS
            ],
            ..Stat::default()
        };
        let frame = phi_rpc::Msg::StatReply(Box::new(s)).encode();
        assert!(
            frame.len() - 4 <= phi_rpc::MAX_FRAME * 3 / 4,
            "{} bytes",
            frame.len()
        );
    }
}
