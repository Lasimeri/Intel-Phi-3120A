//! The card's state for `phitop`: one `Stat` sample from /proc and sysfs.
//! Counters are cumulative; the host differences consecutive samples.
//! See stat.md.

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

/// "1.33" as hundredths; "139.78" as 13978.
fn hundredths(s: &str) -> u64 {
    let mut it = s.trim().splitn(2, '.');
    let whole: u64 = it.next().and_then(num).unwrap_or(0);
    let frac = it.next().unwrap_or("");
    let frac: u64 = match frac.len() {
        0 => 0,
        1 => num::<u64>(frac).unwrap_or(0) * 10,
        _ => num::<u64>(&frac[..2]).unwrap_or(0),
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
            uptime_ms: hundredths(read("/proc/uptime").split_whitespace().next().unwrap_or("0")) * 10,
            ..Stat::default()
        };
        s.cpus = self.cpus();
        s.mem = meminfo();
        loadavg(&mut s);
        self.sensors(&mut s);
        s.disks = diskstats();
        s.nets = netdev();
        self.procs(&mut s);
        s
    }

    fn cpus(&mut self) -> Vec<CpuStat> {
        let mut cpus: Vec<CpuStat> = Vec::with_capacity(self.cores.len().max(64));
        for line in read("/proc/stat").lines() {
            let mut f = line.split_whitespace();
            let Some(name) = f.next() else { continue };
            let Some(n) = name.strip_prefix("cpu").and_then(num::<usize>) else { continue };
            let v: Vec<u64> = f.map(|x| num(x).unwrap_or(0)).collect();
            let at = |i: usize| v.get(i).copied().unwrap_or(0);
            // user nice system idle iowait irq softirq steal
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
        if self.cores.len() < cpus.len() {
            self.cores = (0..cpus.len())
                .map(|n| num(&read(format!("/sys/devices/system/cpu/cpu{n}/topology/core_id"))).unwrap_or(0))
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
        let millideg = |name: String| -> i16 { num::<i64>(&read(h.join(name))).map_or(-1, |v| (v / 1000) as i16) };
        s.temps = (1..=TEMP_CHANNELS).map(|i| millideg(format!("temp{i}_input"))).collect();
        s.temp_peak = (1..=DIE_CHANNELS).map(|i| millideg(format!("temp{i}_max"))).max().unwrap_or(-1);
        s.vcore_mv = num(&read(h.join("in0_input"))).unwrap_or(0);
        s.core_mhz = num(&read(h.join("core_mhz"))).unwrap_or(0);
    }

    fn procs(&mut self, s: &mut Stat) {
        self.samples = self.samples.wrapping_add(1);
        let refresh = self.samples % KTHREAD_EVERY == 1;
        let mut seen: HashMap<u32, u64> = HashMap::with_capacity(self.kthread_ticks.len() + 16);
        let Ok(dir) = fs::read_dir("/proc") else { return };
        for e in dir.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(num::<u32>) else { continue };
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
            let Some(mut p) = parse_stat(pid, &read(e.path().join("stat")), self.page_kb) else { continue };
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
            s.procs.push(p);
        }
        self.kthread_ticks = seen;
    }
}

/// One /proc/[pid]/stat line. The command name is in parentheses and may
/// contain spaces, so the fields are counted from the last ')'.
fn parse_stat(pid: u32, line: &str, page_kb: u64) -> Option<ProcStat> {
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    let comm = line.get(open + 1..close)?.to_string();
    let rest: Vec<&str> = line.get(close + 1..)?.split_whitespace().collect();
    // After comm: state ppid pgrp session tty tpgid flags minflt cminflt
    // majflt cmajflt utime stime cutime cstime priority nice num_threads
    // itrealvalue starttime vsize rss (proc(5)).
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

fn meminfo() -> MemStat {
    let mut m = MemStat::default();
    for line in read("/proc/meminfo").lines() {
        let mut f = line.split_whitespace();
        let (Some(k), Some(v)) = (f.next(), f.next()) else { continue };
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

fn loadavg(s: &mut Stat) {
    let text = read("/proc/loadavg");
    let f: Vec<&str> = text.split_whitespace().collect();
    for (i, l) in s.load.iter_mut().enumerate() {
        *l = f.get(i).map_or(0, |x| hundredths(x).min(u16::MAX as u64) as u16);
    }
    if let Some((r, t)) = f.get(3).and_then(|x| x.split_once('/')) {
        s.running = num(r).unwrap_or(0);
        s.tasks = num(t).unwrap_or(0);
    }
}

/// Block devices served over the ring (phiblk*), bytes read and written.
fn diskstats() -> Vec<DevStat> {
    read("/proc/diskstats")
        .lines()
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

/// Network interfaces except the loopback, bytes received and sent.
fn netdev() -> Vec<DevStat> {
    read("/proc/net/dev")
        .lines()
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
        let k = parse_stat(12, "12 (ksoftirqd/1) S 2 0 0 0 -1 69238848 0 0 0 0 0 3 0 0 20 0 1 0 2 0 0 0", 4).unwrap();
        assert!(k.kthread);
        assert_eq!(k.ticks, 3);
        let odd = parse_stat(7, "7 (a (b) c) R 1 1 1 0 -1 4194560 0 0 0 0 5 6 0 0 20 0 3 0 9 0 250 0", 4).unwrap();
        assert_eq!(odd.comm, "a (b) c");
        assert_eq!(odd.ticks, 11);
        assert_eq!(odd.threads, 3);
        assert_eq!(odd.rss_kb, 1000);
    }

    #[test]
    fn rss_from_status() {
        assert_eq!(vm_rss_kb("Name:\tinit\nVmPeak:\t 1696 kB\nVmRSS:\t     556 kB\nThreads:\t1\n"), Some(556));
        assert_eq!(vm_rss_kb("Name:\tkthreadd\nThreads:\t1\n"), None);
    }

    #[test]
    fn hundredths_of_decimals() {
        assert_eq!(hundredths("1.33"), 133);
        assert_eq!(hundredths("0.5"), 50);
        assert_eq!(hundredths("139.78"), 13978);
        assert_eq!(hundredths("7"), 700);
    }
}
