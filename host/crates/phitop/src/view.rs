//! Rendering: one frame as text with ANSI colours, sized to the terminal.
//! See view.md.

use crate::model::{Derived, Rate, Snapshot};

/// What the keys change.
pub struct Options {
    pub sort_mem: bool,
    pub show_kthreads: bool,
    pub color: bool,
    pub interval_s: f64,
    pub error: Option<String>,
}

/// Eighth blocks: the cell height stands for the load.
const LEVELS: [char; 9] = [
    ' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
];
const LABEL: usize = 7;
const RESET: &str = "\x1b[0m";
const DIM: u8 = 245;

fn level(load: f64) -> char {
    LEVELS[((load.clamp(0.0, 1.0) * 8.0).round() as usize).min(8)]
}

/// 256-colour index for a load: grey, green, yellow, orange, red.
fn color(load: f64) -> u8 {
    if load < 0.02 {
        240
    } else if load < 0.25 {
        34
    } else if load < 0.5 {
        70
    } else if load < 0.75 {
        178
    } else if load < 0.9 {
        208
    } else {
        196
    }
}

fn fg(c: u8) -> String {
    format!("\x1b[38;5;{c}m")
}

/// Bytes per second, in a unit that fits.
pub fn rate(b: f64) -> String {
    if b >= 1e9 {
        format!("{:.2} GB/s", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB/s", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.1} kB/s", b / 1e3)
    } else {
        format!("{b:.0} B/s")
    }
}

fn mib(kb: u64) -> String {
    if kb >= 10 * 1024 * 1024 {
        format!("{:.1} GiB", kb as f64 / (1024.0 * 1024.0))
    } else if kb >= 10 * 1024 {
        format!("{} MiB", kb / 1024)
    } else {
        format!("{kb} KiB")
    }
}

fn bar(frac: f64, width: usize) -> String {
    let filled = ((frac.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    format!("[{}{}]", "#".repeat(filled), ".".repeat(width - filled))
}

fn hms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Truncate to `cols` visible characters, escape sequences not counted,
/// and close any colour that was open.
fn fit(s: &str, cols: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut width = 0;
    let mut in_escape = false;
    let mut coloured = false;
    for ch in s.chars() {
        if in_escape {
            out.push(ch);
            if ch == 'm' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            coloured = true;
            out.push(ch);
            continue;
        }
        if width == cols {
            break;
        }
        out.push(ch);
        width += 1;
    }
    if coloured {
        out.push_str(RESET);
    }
    out
}

fn temps_line(s: &Snapshot) -> String {
    let t = &s.stat.temps;
    let show = |v: i16| if v < 0 { "--".to_string() } else { v.to_string() };
    let die: Vec<String> = t.iter().take(9).map(|v| show(*v)).collect();
    let max = t.iter().take(9).copied().max().unwrap_or(-1);
    let board: Vec<String> = ["inlet", "vccp", "gddr", "gddr vr", "vddg", "tmu"]
        .iter()
        .zip(t.iter().skip(9))
        .filter(|(_, v)| **v > 0)
        .map(|(n, v)| format!("{n} {v}"))
        .collect();
    format!(
        "die   {} C   now {} C  peak {} C   core {} MHz {} mV   {}",
        die.join(" "),
        show(max),
        show(s.stat.temp_peak),
        s.stat.core_mhz,
        s.stat.vcore_mv,
        if board.is_empty() {
            "board sensors: n/a (SMC)".to_string()
        } else {
            board.join("  ")
        }
    )
}

fn rates_line(label: &str, list: &[Rate], a: &str, b: &str) -> String {
    let mut s = format!("{label:<5} ");
    for r in list {
        s.push_str(&format!(" {} {a} {} {b} {}  ", r.name, rate(r.a), rate(r.b)));
    }
    s
}

fn grid(d: &Derived, o: &Options, cols: usize, out: &mut Vec<String>) {
    let ncores = d.cores.len();
    let threads = d.cores.iter().map(|c| c.len()).max().unwrap_or(0);
    let cw = if cols >= LABEL + ncores * 2 { 2 } else { 1 };
    let step = if cw == 2 { 5 } else { 10 };
    let mut head = vec![' '; ncores * cw];
    for c in (0..ncores).step_by(step) {
        for (i, ch) in c.to_string().chars().enumerate() {
            if c * cw + i < head.len() {
                head[c * cw + i] = ch;
            }
        }
    }
    out.push(format!("{:<LABEL$}{}", "core", head.into_iter().collect::<String>()));
    let cell = |load: f64, last: &mut u8, line: &mut String| {
        if o.color {
            let c = color(load);
            if c != *last {
                line.push_str(&fg(c));
                *last = c;
            }
        }
        line.push(level(load));
        if cw == 2 {
            line.push(' ');
        }
    };
    for t in 0..threads {
        let mut line = format!("{:<LABEL$}", format!("t{t}"));
        let mut last = 0;
        for core in &d.cores {
            let load = core.get(t).map_or(0.0, |cpu| d.cpu_load[*cpu]);
            cell(load, &mut last, &mut line);
        }
        out.push(line);
    }
    let mut line = format!("{:<LABEL$}", "mean");
    let mut last = 0;
    for core in &d.cores {
        let load = core.iter().map(|cpu| d.cpu_load[*cpu]).sum::<f64>() / core.len().max(1) as f64;
        cell(load, &mut last, &mut line);
    }
    out.push(line);
}

/// The whole frame, `rows` lines at most, each at most `cols` wide.
pub fn render(cur: &Snapshot, d: &Derived, o: &Options, cols: usize, rows: usize) -> String {
    let s = &cur.stat;
    let mut lines: Vec<String> = Vec::with_capacity(rows);
    let status = match &o.error {
        Some(e) => format!("{}{e}", if o.color { fg(196) } else { String::new() }),
        None => format!("{:.2} s", d.interval),
    };
    lines.push(format!(
        "phitop  Phi 3120A  up {}  load {:.2} {:.2} {:.2}  {} running of {} procs  {} threads {:.1}% busy  {}",
        hms(s.uptime_ms),
        s.load[0] as f64 / 100.0,
        s.load[1] as f64 / 100.0,
        s.load[2] as f64 / 100.0,
        s.running,
        s.nprocs,
        s.cpus.len(),
        d.mean_load * 100.0,
        status
    ));
    lines.push(temps_line(cur));
    let m = &s.mem;
    let used = m.total_kb.saturating_sub(m.avail_kb);
    let swap_used = m.swap_total_kb.saturating_sub(m.swap_free_kb);
    lines.push(format!(
        "mem   {} {} / {}  (cache {})   swap on host RAM {} {} / {}",
        bar(used as f64 / m.total_kb.max(1) as f64, 20),
        mib(used),
        mib(m.total_kb),
        mib(m.buffers_kb + m.cached_kb),
        bar(swap_used as f64 / m.swap_total_kb.max(1) as f64, 10),
        mib(swap_used),
        mib(m.swap_total_kb)
    ));
    lines.push(format!(
        "pcie  to card {} (dma {}, aperture {})   from card {} (dma {}, aperture {})",
        rate(d.dma.0 + d.aperture.0),
        rate(d.dma.0),
        rate(d.aperture.0),
        rate(d.dma.1 + d.aperture.1),
        rate(d.dma.1),
        rate(d.aperture.1)
    ));
    lines.push(format!(
        "{}{}",
        rates_line("card", &d.disks, "read", "write"),
        rates_line("", &d.nets, "rx", "tx")
    ));
    lines.push(String::new());
    grid(d, o, cols, &mut lines);
    lines.push(String::new());
    lines.push(format!(
        "{:>7} {} {:>7} {:>9} {:>4}  {}   ({} kernel threads, {} active shown)",
        "PID",
        "S",
        "CPU%",
        "RSS",
        "THR",
        "COMMAND",
        s.nkthreads,
        if o.show_kthreads { "the" } else { "none" }
    ));
    let mut procs: Vec<&crate::model::ProcRow> = d.procs.iter().filter(|p| o.show_kthreads || !p.kthread).collect();
    if o.sort_mem {
        procs.sort_by(|a, b| b.rss_kb.cmp(&a.rss_kb).then(b.cpu.total_cmp(&a.cpu)));
    } else {
        procs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.rss_kb.cmp(&a.rss_kb)));
    }
    let footer = format!(
        "q quit   c/m sort by cpu/mem ({})   k kernel threads ({})   +/- interval",
        if o.sort_mem { "mem" } else { "cpu" },
        if o.show_kthreads { "shown when active" } else { "hidden" }
    );
    let room = rows.saturating_sub(lines.len() + 1);
    for p in procs.iter().take(room) {
        let name = if p.kthread { format!("[{}]", p.comm) } else { p.comm.clone() };
        let row = format!(
            "{:>7} {} {:>7.1} {:>9} {:>4}  {}",
            p.pid,
            p.state,
            p.cpu,
            mib(p.rss_kb),
            p.threads,
            name
        );
        lines.push(if p.kthread && o.color { format!("{}{row}", fg(DIM)) } else { row });
    }
    while lines.len() + 1 < rows {
        lines.push(String::new());
    }
    lines.push(footer);
    let mut out = String::with_capacity(cols * rows * 2);
    if o.color {
        out.push_str("\x1b[H");
    }
    for (i, l) in lines.iter().take(rows).enumerate() {
        out.push_str(&fit(l, cols));
        if o.color {
            out.push_str("\x1b[K");
        }
        if i + 1 < rows || !o.color {
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_counts_visible_characters_only() {
        let s = format!("{}abcdef{}", fg(34), RESET);
        let f = fit(&s, 3);
        assert!(f.starts_with(&fg(34)));
        assert!(f.contains("abc"));
        assert!(!f.contains("abcd"));
        assert!(f.ends_with(RESET));
        assert_eq!(fit("plain text", 5), "plain");
        assert_eq!(fit("\u{2588}\u{2588}\u{2588}", 2), "\u{2588}\u{2588}");
    }

    #[test]
    fn units() {
        assert_eq!(rate(0.0), "0 B/s");
        assert_eq!(rate(1536.0), "1.5 kB/s");
        assert_eq!(rate(846e6), "846.0 MB/s");
        assert_eq!(rate(3.58e9), "3.58 GB/s");
        assert_eq!(mib(5_805_088), "5669 MiB");
        assert_eq!(mib(556), "556 KiB");
        assert_eq!(mib(0), "0 KiB");
        assert_eq!(mib(16 << 20), "16.0 GiB");
        assert_eq!(hms(3_723_000), "1:02:03");
        assert_eq!(level(0.0), ' ');
        assert_eq!(level(1.0), '\u{2588}');
        assert_eq!(bar(0.5, 4), "[##..]");
    }
}
