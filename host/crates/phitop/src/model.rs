//! From consecutive samples to what the screen shows: per-thread load, the
//! core layout, transfer rates and per-process CPU shares. See model.md.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use phi_rpc::{DevStat, Stat, Traffic};

/// USER_HZ: /proc/stat and /proc/[pid]/stat count in these ticks.
const TICKS_PER_SEC: f64 = 100.0;
/// A process unseen for this long is forgotten.
const FORGET_AFTER: Duration = Duration::from_secs(120);

/// One sample and when the host received it.
pub struct Snapshot {
    pub stat: Stat,
    pub traffic: Traffic,
    pub at: Instant,
}

/// Two rates for one device or path, in bytes per second.
pub struct Rate {
    pub name: String,
    pub a: f64,
    pub b: f64,
}

/// One row of the process table.
pub struct ProcRow {
    pub pid: u32,
    pub state: char,
    pub kthread: bool,
    /// CPU share in percent of one hardware thread; a process with many
    /// threads can exceed 100.
    pub cpu: f64,
    pub rss_kb: u64,
    pub threads: u32,
    pub comm: String,
}

/// Everything derived from the last two samples.
pub struct Derived {
    /// Seconds between the two samples, host clock.
    pub interval: f64,
    /// Load of each CPU over the interval, 0 to 1, in /proc/stat order.
    pub cpu_load: Vec<f64>,
    /// CPU indices of each physical core: cores in core_id order, threads
    /// in CPU order.
    pub cores: Vec<Vec<usize>>,
    /// Mean of `cpu_load`.
    pub mean_load: f64,
    /// DMA and aperture rates: to the card, from the card.
    pub dma: (f64, f64),
    pub aperture: (f64, f64),
    pub disks: Vec<Rate>,
    pub nets: Vec<Rate>,
    pub procs: Vec<ProcRow>,
}

/// The previous sample and per-process history.
#[derive(Default)]
pub struct Model {
    prev: Option<Snapshot>,
    /// CPU ticks and the time they were reported, per pid.
    seen: HashMap<u32, (u64, Instant)>,
}

impl Model {
    /// The latest sample.
    pub fn current(&self) -> Option<&Snapshot> {
        self.prev.as_ref()
    }

    /// Fold in a sample; None until there are two.
    pub fn update(&mut self, cur: Snapshot) -> Option<Derived> {
        let derived = self.prev.as_ref().map(|prev| self.derive(prev, &cur));
        self.remember(&cur);
        self.prev = Some(cur);
        derived
    }

    fn remember(&mut self, cur: &Snapshot) {
        for p in &cur.stat.procs {
            self.seen.insert(p.pid, (p.ticks, cur.at));
        }
        self.seen.retain(|_, (_, at)| cur.at.duration_since(*at) < FORGET_AFTER);
    }

    fn derive(&self, prev: &Snapshot, cur: &Snapshot) -> Derived {
        let interval = cur.at.duration_since(prev.at).as_secs_f64().max(1e-3);
        let mut cpu_load = Vec::with_capacity(cur.stat.cpus.len());
        for (i, c) in cur.stat.cpus.iter().enumerate() {
            let p = prev.stat.cpus.get(i).copied().unwrap_or_default();
            let busy = c.busy.wrapping_sub(p.busy) as f64;
            let idle = c.idle.wrapping_sub(p.idle) as f64;
            cpu_load.push(if busy + idle > 0.0 { busy / (busy + idle) } else { 0.0 });
        }
        let mut by_core: Vec<(u16, Vec<usize>)> = Vec::new();
        for (i, c) in cur.stat.cpus.iter().enumerate() {
            match by_core.iter_mut().find(|(id, _)| *id == c.core) {
                Some((_, v)) => v.push(i),
                None => by_core.push((c.core, vec![i])),
            }
        }
        by_core.sort_by_key(|(id, _)| *id);
        let cores = by_core.into_iter().map(|(_, v)| v).collect();
        let n = cpu_load.len().max(1) as f64;
        let mean_load = cpu_load.iter().sum::<f64>() / n;
        let rate = |now: u64, before: u64| now.wrapping_sub(before) as f64 / interval;
        let t = (&cur.traffic, &prev.traffic);
        let dma = (rate(t.0.dma_to_card, t.1.dma_to_card), rate(t.0.dma_from_card, t.1.dma_from_card));
        let aperture = (
            rate(t.0.aperture_to_card, t.1.aperture_to_card),
            rate(t.0.aperture_from_card, t.1.aperture_from_card),
        );
        let dev_rates = |now: &[DevStat], before: &[DevStat]| -> Vec<Rate> {
            let mut rates: Vec<Rate> = now
                .iter()
                .map(|d| {
                    let b = before.iter().find(|b| b.name == d.name);
                    Rate {
                        name: d.name.clone(),
                        a: rate(d.read, b.map_or(d.read, |b| b.read)),
                        b: rate(d.written, b.map_or(d.written, |b| b.written)),
                    }
                })
                .collect();
            rates.sort_by(|a, b| a.name.cmp(&b.name));
            rates
        };
        let disks = dev_rates(&cur.stat.disks, &prev.stat.disks);
        let nets = dev_rates(&cur.stat.nets, &prev.stat.nets);
        // A process's share comes from its ticks since it was last reported,
        // over that span: a kernel thread the agent left out while idle gets
        // its burst averaged over the gap, a new process shows zero until
        // its second report.
        let procs = cur
            .stat
            .procs
            .iter()
            .map(|p| {
                let cpu = match self.seen.get(&p.pid) {
                    Some((ticks, at)) if p.ticks >= *ticks && cur.at > *at => {
                        let span = cur.at.duration_since(*at).as_secs_f64().max(1e-3);
                        (p.ticks - ticks) as f64 / TICKS_PER_SEC / span * 100.0
                    }
                    _ => 0.0,
                };
                ProcRow {
                    pid: p.pid,
                    state: p.state as char,
                    kthread: p.kthread,
                    cpu,
                    rss_kb: p.rss_kb,
                    threads: p.threads,
                    comm: p.comm.clone(),
                }
            })
            .collect();
        Derived {
            interval,
            cpu_load,
            cores,
            mean_load,
            dma,
            aperture,
            disks,
            nets,
            procs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phi_rpc::{CpuStat, ProcStat};

    fn snap(ticks: u32, pticks: u64, at: Instant) -> Snapshot {
        Snapshot {
            stat: Stat {
                cpus: vec![
                    CpuStat {
                        core: 3,
                        busy: ticks,
                        idle: 100 + ticks,
                    },
                    CpuStat {
                        core: 0,
                        busy: 0,
                        idle: 100 + ticks,
                    },
                    CpuStat {
                        core: 3,
                        busy: 0,
                        idle: 100 + ticks,
                    },
                ],
                procs: vec![ProcStat {
                    pid: 9,
                    ticks: pticks,
                    comm: "x".into(),
                    ..ProcStat::default()
                }],
                ..Stat::default()
            },
            traffic: Traffic {
                dma_from_card: ticks as u64 * 1000,
                ..Traffic::default()
            },
            at,
        }
    }

    #[test]
    fn loads_cores_rates_and_shares() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(2);
        let mut m = Model::default();
        assert!(m.update(snap(0, 0, t0)).is_none());
        let d = m.update(snap(50, 100, t1)).unwrap();
        assert!((d.interval - 2.0).abs() < 1e-6);
        assert!((d.cpu_load[0] - 0.5).abs() < 1e-9);
        assert_eq!(d.cpu_load[1], 0.0);
        assert_eq!(d.cores, vec![vec![1], vec![0, 2]]);
        assert!((d.dma.1 - 25_000.0).abs() < 1e-6);
        // 100 ticks in 2 s = 50 percent of one thread.
        assert!((d.procs[0].cpu - 50.0).abs() < 1e-6);
    }
}
