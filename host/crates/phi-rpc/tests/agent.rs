//! The card agent (`card/agent`, x86-64 assembly) against this crate: the
//! agent is built with its own `build.sh --out`, run on the host (its
//! instructions are the Knights Corner subset, which the host executes)
//! with a socket as its device (`--device -`), and driven with frames
//! encoded here. Every reply is decoded here, so a disagreement on the
//! wire shows up as a failed decode. The sampler is pointed at fixture
//! trees (`--root`) so that a `StatReply` can be checked field by field.
//! See agent.md.

use std::fs;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use phi_rpc::{CpuStat, Decoder, DevStat, MemStat, Msg, ProcStat, Stat, MAX_FRAME};

/// The agent, assembled once per test run.
fn binary() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../card/agent/build.sh");
        let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("agent");
        let st = Command::new("bash")
            .arg(&script)
            .arg("--out")
            .arg(&out)
            .status()
            .expect("run card/agent/build.sh");
        assert!(st.success(), "card/agent/build.sh --out failed");
        out.join("phi-agent")
    })
}

/// A scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("agent-{name}"));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

struct Agent {
    child: Child,
    sock: UnixStream,
    dec: Decoder,
    log: PathBuf,
}

impl Agent {
    fn start(name: &str, root: Option<&Path>) -> Agent {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let log = scratch(&format!("{name}-log")).join("stderr");
        let mut cmd = Command::new(binary());
        cmd.arg("--device").arg("-");
        if let Some(r) = root {
            cmd.arg("--root").arg(r);
        }
        cmd.stdin(Stdio::from(OwnedFd::from(theirs)))
            .stdout(Stdio::null())
            .stderr(Stdio::from(fs::File::create(&log).unwrap()));
        let child = cmd.spawn().expect("start the agent");
        ours.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        Agent {
            child,
            sock: ours,
            dec: Decoder::new(),
            log,
        }
    }

    fn send(&mut self, m: &Msg) {
        self.sock.write_all(&m.encode()).unwrap();
    }

    fn send_raw(&mut self, b: &[u8]) {
        self.sock.write_all(b).unwrap();
    }

    fn recv(&mut self) -> Msg {
        let mut buf = vec![0u8; 65536];
        loop {
            if let Some(m) = self.dec.next_frame().expect("the agent sent a frame phi-rpc rejects") {
                return m;
            }
            let n = self.sock.read(&mut buf).expect("read from the agent");
            assert!(n > 0, "the agent closed its end; log:\n{}", self.log());
            self.dec.push(&buf[..n]);
        }
    }

    /// Frames until the one that ends a session (Exit, Error, GetEnd).
    fn session(&mut self) -> Vec<Msg> {
        let mut v = Vec::new();
        loop {
            let m = self.recv();
            let end = matches!(m, Msg::Exit(_) | Msg::Error(_) | Msg::GetEnd { .. });
            v.push(m);
            if end {
                return v;
            }
        }
    }

    fn ping(&mut self) {
        self.send(&Msg::Ping);
        assert_eq!(self.recv(), Msg::Pong { version: "0.2.0".into() });
    }

    fn log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn exec(argv: &[&str], env: &[(&str, &str)], cwd: Option<&str>) -> Msg {
    Msg::Exec {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        env: env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        cwd: cwd.map(str::to_string),
    }
}

const PATH: (&str, &str) = ("PATH", "/usr/local/bin:/usr/bin:/bin");

/// A session's output split into standard output, standard error and the
/// final frame.
fn streams(frames: Vec<Msg>) -> (Vec<u8>, Vec<u8>, Msg) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut last = None;
    for m in frames {
        match m {
            Msg::Stdout(d) => out.extend_from_slice(&d),
            Msg::Stderr(d) => err.extend_from_slice(&d),
            other => last = Some(other),
        }
    }
    (out, err, last.unwrap())
}

#[test]
fn passes_the_knc_audit() {
    let data = fs::read(binary()).unwrap();
    let report = phi_isa_audit::audit_elf(&data).unwrap();
    assert!(report.instructions > 1000);
    assert!(
        report.hits.is_empty(),
        "instructions Knights Corner lacks: {:?}",
        report.hits.iter().map(|h| h.text.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn ping_and_startup_line() {
    let mut a = Agent::start("ping", None);
    a.ping();
    assert!(a.log().starts_with("phi-agent 0.2.0 listening on -\n"), "{}", a.log());
}

#[test]
fn exec_relays_streams_and_status() {
    let mut a = Agent::start("streams", None);
    a.send(&exec(&["/bin/sh", "-c", "echo out; echo err >&2; exit 3"], &[PATH], None));
    let (out, err, last) = streams(a.session());
    assert_eq!(out, b"out\n");
    assert_eq!(err, b"err\n");
    assert_eq!(last, Msg::Exit(3));
    a.send(&exec(&["/bin/sh", "-c", "kill -9 $$"], &[PATH], None));
    assert_eq!(streams(a.session()).2, Msg::Exit(128 + 9));
}

#[test]
fn exec_input_by_path_lookup() {
    let mut a = Agent::start("stdin", None);
    a.send(&exec(&["cat"], &[PATH], None));
    a.send(&Msg::Stdin(b"hello ".to_vec()));
    a.send(&Msg::Stdin(b"world".to_vec()));
    a.send(&Msg::StdinEof);
    let (out, _, last) = streams(a.session());
    assert_eq!(out, b"hello world");
    assert_eq!(last, Msg::Exit(0));
}

#[test]
fn exec_large_input_and_output() {
    // More than the input backlog in both directions, written from a
    // second thread as the daemon's relay does, so neither side waits on
    // the other.
    let mut a = Agent::start("large", None);
    let data: Vec<u8> = (0..3u32 << 20).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
    a.send(&exec(&["cat"], &[PATH], None));
    let mut w = a.sock.try_clone().unwrap();
    let input = data.clone();
    let t = std::thread::spawn(move || {
        for c in input.chunks(65536) {
            w.write_all(&Msg::Stdin(c.to_vec()).encode()).unwrap();
        }
        w.write_all(&Msg::StdinEof.encode()).unwrap();
    });
    let frames = a.session();
    t.join().unwrap();
    assert!(frames.iter().all(|m| match m {
        Msg::Stdout(d) => d.len() <= 32768,
        _ => true,
    }));
    let (out, _, last) = streams(frames);
    assert_eq!(last, Msg::Exit(0));
    assert!(out == data, "{} bytes back of {}", out.len(), data.len());
}

#[test]
fn exec_failures_are_errors() {
    let mut a = Agent::start("fail", None);
    a.send(&exec(&["no-such-program-xyz"], &[("PATH", "/nonexistent:/usr/bin")], None));
    assert_eq!(
        a.session(),
        vec![Msg::Error("no-such-program-xyz: No such file or directory (os error 2)".into())]
    );
    a.send(&exec(&[], &[], None));
    assert_eq!(a.session(), vec![Msg::Error("empty argv".into())]);
    a.send(&exec(&["/bin/echo", "a\0b"], &[], None));
    assert_eq!(a.session(), vec![Msg::Error("/bin/echo: nul byte found in provided data".into())]);
    a.send(&exec(&["/bin/pwd"], &[], Some("/nonexistent-dir")));
    assert_eq!(
        a.session(),
        vec![Msg::Error("/bin/pwd: No such file or directory (os error 2)".into())]
    );
    let d = scratch("noexec");
    let f = d.join("script");
    fs::write(&f, "not a program").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o644)).unwrap();
    a.send(&exec(&[f.to_str().unwrap()], &[], None));
    assert_eq!(
        a.session(),
        vec![Msg::Error(format!("{}: Permission denied (os error 13)", f.display()))]
    );
    a.ping();
}

#[test]
fn exec_cwd_and_environment() {
    let mut a = Agent::start("env", None);
    a.send(&exec(&["/bin/pwd"], &[], Some("/tmp")));
    assert_eq!(streams(a.session()).0, b"/tmp\n");
    // One entry per key, the last value winning, sorted by key: the
    // environment Rust's Command handed over.
    a.send(&exec(
        &["/usr/bin/env"],
        &[("B", "1"), ("A", "2"), ("B", "3"), ("AB", "4"), ("a", "5")],
        None,
    ));
    assert_eq!(streams(a.session()).0, b"A=2\nAB=4\nB=3\na=5\n");
    // No PATH: musl's default search path.
    a.send(&exec(&["env"], &[], None));
    let (out, _, last) = streams(a.session());
    assert_eq!((out, last), (Vec::new(), Msg::Exit(0)));
}

#[test]
fn stat_is_answered_during_a_command() {
    let mut a = Agent::start("statcmd", None);
    a.send(&exec(&["/bin/sh", "-c", "sleep 0.5; echo done"], &[PATH], None));
    std::thread::sleep(Duration::from_millis(100));
    a.send(&Msg::Stat);
    match a.recv() {
        Msg::StatReply(s) => assert!(s.nprocs > 0),
        other => panic!("{other:?}"),
    }
    let (out, _, last) = streams(a.session());
    assert_eq!((out, last), (b"done\n".to_vec(), Msg::Exit(0)));
}

#[test]
fn background_children_do_not_hold_the_session() {
    let mut a = Agent::start("bg", None);
    let t0 = Instant::now();
    a.send(&exec(&["/bin/sh", "-c", "(sleep 3; echo late) & echo early"], &[PATH], None));
    let (out, _, last) = streams(a.session());
    let took = t0.elapsed();
    assert_eq!((out, last), (b"early\n".to_vec(), Msg::Exit(0)));
    assert!(took < Duration::from_secs(2), "{took:?}");
    // Nothing of the old session follows the next one's reply.
    a.ping();
}

#[test]
fn put_and_get_round_trip() {
    let mut a = Agent::start("files", None);
    let d = scratch("files");
    let path = d.join("f.bin");
    let data: Vec<u8> = (0..100_000u32).map(|i| (i * 7 + 3) as u8).collect();
    a.send(&Msg::PutOpen {
        path: path.to_str().unwrap().into(),
        mode: 0o640,
    });
    for c in data.chunks(30_000) {
        a.send(&Msg::PutData(c.to_vec()));
    }
    a.send(&Msg::Stat);
    a.send(&Msg::PutClose);
    assert!(matches!(a.recv(), Msg::StatReply(_)));
    assert_eq!(a.session(), vec![Msg::Exit(0)]);
    assert_eq!(fs::read(&path).unwrap(), data);
    // Exactly the mode asked for, whatever the umask.
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o7777, 0o640);

    a.send(&Msg::Get {
        path: path.to_str().unwrap().into(),
    });
    let frames = a.session();
    let mut back = Vec::new();
    for m in &frames[..frames.len() - 1] {
        match m {
            Msg::GetData(d) => {
                assert!(d.len() <= 32768);
                back.extend_from_slice(d);
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(frames.last(), Some(&Msg::GetEnd { size: 100_000 }));
    assert_eq!(back, data);
}

#[test]
fn file_failures_are_errors() {
    let mut a = Agent::start("filefail", None);
    a.send(&Msg::PutOpen {
        path: "/nonexistent-dir/x".into(),
        mode: 0o644,
    });
    assert_eq!(
        a.session(),
        vec![Msg::Error("/nonexistent-dir/x: No such file or directory (os error 2)".into())]
    );
    // The rest of the failed put is dropped without a reply.
    a.send(&Msg::PutData(vec![1, 2, 3]));
    a.send(&Msg::PutClose);
    a.ping();
    a.send(&Msg::Get { path: "/tmp".into() });
    assert_eq!(a.session(), vec![Msg::Error("/tmp: Is a directory (os error 21)".into())]);
    a.send(&Msg::Get { path: "a\0b".into() });
    assert_eq!(
        a.session(),
        vec![Msg::Error("a\0b: file name contained an unexpected NUL byte".into())]
    );
}

#[test]
fn stray_and_bad_frames() {
    let mut a = Agent::start("bad", None);
    a.send(&Msg::Stdout(b"x".to_vec()));
    assert_eq!(a.session(), vec![Msg::Error("unexpected Stdout outside a session".into())]);
    a.send(&Msg::Stdin(b"late".to_vec()));
    a.send(&Msg::StdinEof);
    // an unknown tag and a short body cost one frame each
    a.send_raw(&[3, 0, 0, 0, 200, 1, 2]);
    a.send_raw(&[2, 0, 0, 0, 10, 5]);
    // a string that is not UTF-8
    a.send_raw(&[4, 0, 0, 0, 13, 1, 0, 0xff]);
    a.ping();
    // a bad length loses what is buffered with it
    let mut b = vec![0, 0, 0, 0];
    b.extend_from_slice(&Msg::Ping.encode());
    a.send_raw(&b);
    std::thread::sleep(Duration::from_millis(200));
    a.ping();
    // an over-long frame
    a.send_raw(&((MAX_FRAME as u32) + 1).to_le_bytes());
    std::thread::sleep(Duration::from_millis(200));
    a.ping();
    let log = a.log();
    for want in [
        "phi-agent: bad frame from the host: unknown message tag 0xc8\n",
        "phi-agent: bad frame from the host: malformed frame body\n",
        "phi-agent: bad frame from the host: frame length 0 out of range\n",
        "phi-agent: bad frame from the host: frame length 1048577 out of range\n",
    ] {
        assert!(log.contains(want), "{want:?} not in\n{log}");
    }
    assert_eq!(log.matches("malformed frame body").count(), 2);
}

/// Write `text` at `root/rel`, making directories.
fn put(root: &Path, rel: &str, text: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

const PROC_STAT: &str = "cpu  392024 0 4204 611223798 210 0 31 0 0 0\n\
cpu0 1731 0 117 2679217 1 0 14 0 0 0\n\
cpu1 1730 0 117 2679219 0 0 6 0 0 0\n\
cpu3 5 4 3 100 20 1 1 1 9 9\n\
intr 12345 0 0\n\
ctxt 4711\n";

fn fixture(name: &str) -> PathBuf {
    let r = scratch(name);
    put(&r, "proc/uptime", b"26811.34 1500000.12\n");
    put(&r, "proc/stat", PROC_STAT.as_bytes());
    for (n, core) in [(0, "0"), (1, "0"), (2, "1"), (3, "x")] {
        put(
            &r,
            &format!("sys/devices/system/cpu/cpu{n}/topology/core_id"),
            format!("{core}\n").as_bytes(),
        );
    }
    put(
        &r,
        "proc/meminfo",
        b"MemTotal:        5805088 kB\nMemFree:         5398476 kB\nMemAvailable:    5379572 kB\n\
Buffers:            8916 kB\nCached:            18912 kB\nSwapCached:            0 kB\n\
SwapTotal:       4194300 kB\nSwapFree:        4194300 kB\n",
    );
    put(&r, "proc/loadavg", b"1000.00 1.50 0.07 3/1529 2131\n");
    put(&r, "sys/class/hwmon/hwmon0/name", b"acpitz\n");
    put(&r, "sys/class/hwmon/hwmon1/name", b"knc\n");
    for i in 1..=15 {
        if i <= 7 {
            put(
                &r,
                &format!("sys/class/hwmon/hwmon1/temp{i}_input"),
                format!("{}\n", 50_000 + i * 999).as_bytes(),
            );
        }
        if i <= 9 {
            put(
                &r,
                &format!("sys/class/hwmon/hwmon1/temp{i}_max"),
                format!("{}\n", 60_000 + i * 100).as_bytes(),
            );
        }
    }
    put(&r, "sys/class/hwmon/hwmon1/temp8_input", b"-1500\n");
    put(&r, "sys/class/hwmon/hwmon1/in0_input", b"1003\n");
    put(&r, "sys/class/hwmon/hwmon1/core_mhz", b"1100\n");
    put(
        &r,
        "proc/diskstats",
        b"   7       0 loop0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n \
 251       0 phiblk0 7328 140 3672642 17089 3659 103 3671304 266084 0 7624 283303 0 0 0 0 16 129\n \
 251      16 phiblk1 3 0 24 5 2 0 8 1 0 5 6 0 0 0 0 1 0\n \
 251      32 phiblk2 short\n",
    );
    put(
        &r,
        "proc/net/dev",
        b"Inter-|   Receive                                                |  Transmit\n \
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    \
lo:       0       0    0    0    0     0          0         0        0       0    0    0    0     0       0          0\n  \
phi0:    7150      35    0    0    0     0          0         0     7151      67    0    0    0     0       0          0\n",
    );
    // pid 1: a user process whose stat rss reads low; status has VmRSS
    put(
        &r,
        "proc/1/stat",
        b"1 (init) S 0 0 0 0 -1 4194304 239 1184 0 0 0 103 0 42 20 0 1 0 57 1736704 139 18446744073709551615\n",
    );
    put(
        &r,
        "proc/1/status",
        b"Name:\tinit\nVmPeak:\t 1696 kB\nVmRSS:\t     556 kB\nThreads:\t1\n",
    );
    // pid 7: parentheses and spaces in the name, no VmRSS line
    put(
        &r,
        "proc/7/stat",
        b"7 (a (b) c) R 1 1 1 0 -1 4194560 0 0 0 0 5 6 0 0 20 0 3 0 9 0 250 0\n",
    );
    put(&r, "proc/7/status", b"Name:\ta (b) c\n");
    // pid 12: a kernel thread
    put(
        &r,
        "proc/12/stat",
        b"12 (ksoftirqd/1) S 2 0 0 0 -1 69238848 0 0 0 0 0 3 0 0 20 0 1 0 2 0 0 0\n",
    );
    // pid 13: a name that is not UTF-8 makes the file unreadable as text
    put(
        &r,
        "proc/13/stat",
        b"13 (bad\xff) S 2 0 0 0 -1 69238848 0 0 0 0 0 3 0 0 20 0 1 0 2 0 0 0\n",
    );
    // pid 14 vanished between readdir and read; "self" is not a pid
    fs::create_dir_all(r.join("proc/14")).unwrap();
    fs::create_dir_all(r.join("proc/self")).unwrap();
    r
}

fn sample(a: &mut Agent) -> Stat {
    a.send(&Msg::Stat);
    match a.recv() {
        Msg::StatReply(s) => *s,
        other => panic!("{other:?}"),
    }
}

fn by_pid(mut v: Vec<ProcStat>) -> Vec<ProcStat> {
    v.sort_by_key(|p| p.pid);
    v
}

#[test]
fn stat_from_a_fixture() {
    let r = fixture("stat");
    let mut a = Agent::start("stat", Some(&r));
    let s = sample(&mut a);
    let init = ProcStat {
        pid: 1,
        state: b'S',
        kthread: false,
        ticks: 103,
        rss_kb: 556,
        threads: 1,
        comm: "init".into(),
    };
    let odd = ProcStat {
        pid: 7,
        state: b'R',
        kthread: false,
        ticks: 11,
        rss_kb: 1000,
        threads: 3,
        comm: "a (b) c".into(),
    };
    let kthread = ProcStat {
        pid: 12,
        state: b'S',
        kthread: true,
        ticks: 3,
        rss_kb: 0,
        threads: 1,
        comm: "ksoftirqd/1".into(),
    };
    let want = Stat {
        uptime_ms: 26_811_340,
        cpus: vec![
            CpuStat {
                core: 0,
                busy: 1731 + 117 + 14,
                idle: 2679217 + 1,
            },
            CpuStat {
                core: 0,
                busy: 1730 + 117 + 6,
                idle: 2679219,
            },
            CpuStat { core: 1, busy: 0, idle: 0 },
            CpuStat {
                core: 0,
                busy: 5 + 4 + 3 + 1 + 1 + 1,
                idle: 120,
            },
        ],
        mem: MemStat {
            total_kb: 5_805_088,
            avail_kb: 5_379_572,
            buffers_kb: 8916,
            cached_kb: 18912,
            swap_total_kb: 4_194_300,
            swap_free_kb: 4_194_300,
        },
        load: [u16::MAX, 150, 7],
        running: 3,
        tasks: 1529,
        temps: (1..=15)
            .map(|i: i16| match i {
                1..=7 => 50 + (i * 999) / 1000,
                8 => -1,
                _ => -1,
            })
            .collect(),
        temp_peak: 60,
        vcore_mv: 1003,
        core_mhz: 1100,
        disks: vec![
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
        ],
        nets: vec![DevStat {
            name: "phi0".into(),
            read: 7150,
            written: 7151,
        }],
        procs: vec![init.clone(), odd.clone(), kthread.clone()],
        nprocs: 3,
        nkthreads: 1,
    };
    let mut got = s.clone();
    got.procs = by_pid(got.procs);
    assert_eq!(got, want);

    // Samples 2 to 4: the known kernel thread is counted, not read again,
    // so a change to it goes unseen; user processes are read every time.
    put(
        &r,
        "proc/12/stat",
        b"12 (ksoftirqd/1) S 2 0 0 0 -1 69238848 0 0 0 0 0 9 0 0 20 0 1 0 2 0 0 0\n",
    );
    put(&r, "proc/1/status", b"VmRSS:\t 600 kB\n");
    for _ in 2..=4 {
        let s = sample(&mut a);
        assert_eq!((s.nprocs, s.nkthreads), (3, 1));
        let procs = by_pid(s.procs);
        assert_eq!(procs.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![1, 7]);
        assert_eq!(procs[0].rss_kb, 600);
    }
    // Sample 5 refreshes: the thread moved, so it is listed with its ticks.
    let procs = by_pid(sample(&mut a).procs);
    assert_eq!(
        procs.iter().map(|p| (p.pid, p.ticks)).collect::<Vec<_>>(),
        vec![(1, 103), (7, 11), (12, 9)]
    );
    // Sample 6 is not a refresh: the thread is known again, and unlisted.
    let procs = by_pid(sample(&mut a).procs);
    assert_eq!(procs.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![1, 7]);
}

#[test]
fn stat_without_hwmon_or_files() {
    let r = scratch("stat-empty");
    let mut a = Agent::start("stat-empty", Some(&r));
    // No /proc at all: every part empty or zero, the peak 0 as the Rust
    // agent's default.
    assert_eq!(sample(&mut a), Stat::default());
}

#[test]
fn stat_of_this_host() {
    let mut a = Agent::start("stat-host", None);
    let s = sample(&mut a);
    let cpus = fs::read_to_string("/proc/stat")
        .unwrap()
        .lines()
        .filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(u8::is_ascii_digit))
        .count();
    assert_eq!(s.cpus.len(), cpus);
    assert!(s.mem.total_kb > 0 && s.uptime_ms > 0 && s.nprocs > 0 && s.tasks > 0);
    let me = s.procs.iter().find(|p| p.pid == a.child.id()).expect("the agent lists itself");
    assert_eq!(me.comm, "phi-agent");
    assert!(!me.kthread && me.rss_kb > 0 && me.threads == 1);
}
