# stat.S: the card's state as one sample

Answers the host's `Stat` request (`phi-rpc`) with everything `phitop`
shows, read from the card's own /proc and sysfs and written into the
`StatReply` frame in wire order as it is read (`put_stat` in
`host/crates/phi-rpc/src/lib.rs`):

| field | source |
| --- | --- |
| uptime | `/proc/uptime`, the first number, in milliseconds |
| per-CPU busy and idle ticks, core id | `/proc/stat` cpuN lines (busy = user nice system irq softirq steal, idle = idle iowait, `fs/proc/stat.c`; each the low 32 bits); `/sys/devices/system/cpu/cpuN/topology/core_id`, read again whenever more CPUs appear |
| memory and swap | `/proc/meminfo`: MemTotal, MemAvailable, Buffers, Cached, SwapTotal, SwapFree |
| load averages, runnable and total tasks | `/proc/loadavg`, averages in hundredths capped at the wire's 655.35 |
| temperatures, peak die temperature, core voltage and clock | the hwmon device named `knc` (kernel patch 0027): `temp1..15_input`, the highest of `temp1..9_max`, `in0_input`, `core_mhz`; whole degrees, truncated, -1 for a channel that does not read |
| bytes read and written per block device | `/proc/diskstats`, devices named `phiblk*`, fields 6 and 10 times 512 (`Documentation/admin-guide/iostats.rst`) |
| bytes received and sent per interface | `/proc/net/dev`, every interface but `lo`, the first and ninth counters |
| processes | `/proc/[pid]/stat`: state, PF_KTHREAD, utime plus stime, threads, command name, rss; `/proc/[pid]/status` VmRSS for user processes |

Counters are sent as they are; the host differences two samples for
rates and shares (`host/asm/phitop/model.md`).

## The parsing rules

These are the Rust agent's, which parsed with `str::parse` and
`fs::read_to_string` (its `src/stat.rs`, removed with the port), kept rule
for rule because phitop's display was built on them:

- A file that cannot be read, or is not UTF-8, reads as empty text
  (`read_to_string(..).unwrap_or_default()`). That includes a process
  whose command name is not UTF-8: its stat file reads as empty, so it is
  neither counted nor listed, and no invalid string reaches the wire
  (where phitop's decoder would reject the whole sample).
- Numbers parse as Rust's `from_str` does: an optional `+` (and `-` for
  the signed temperature), digits only, no overflow; anything else reads
  as 0, or leaves the entry out where the Rust code used `?` (a disk or
  interface whose counters do not parse).
- Lines split on `\n` with an optional `\r` before it; fields on ASCII
  whitespace (the only whitespace /proc and sysfs write).
- The command name is taken between the first "(" and the last ")" of the
  stat line (proc(5)), so a name holding spaces or parentheses parses.
- Kernel threads: the card runs about 1600 of them, almost all asleep.
  Reading their stat files every sample cost a sixth of a hardware thread
  (measured 2026-09-17), so their tick counts are kept (a table sorted by
  pid, looked up by binary search) and a known kernel thread is re-read
  only every fourth sample, and listed only when its count moved. User
  processes and new pids are read and listed every sample; `nprocs` and
  `nkthreads` count everything.
- VmRSS: the stat line's rss is the global part of a per-CPU counter whose
  batch on 228 CPUs is 456 pages, so a small process reads 0 there
  (measured 2026-09-17); the first `VmRSS:` line of status replaces it for
  user processes.

## Limits

At most 16384 processes are listed (the Rust agent's `MAX_PROCS`), and an
entry (a process or a device) is left out if it would take the frame body
past `MAX_FRAME`. The Rust agent relied on `MAX_PROCS` alone, which
kernel-thread names of up to 63 bytes (`proc_task_name` in
`fs/proc/array.c`) could have passed; here the frame always decodes. CPUs
numbered past 4096 are skipped (the card has 228). System files are read up
to 1 MiB, per-process files up to 64 KiB.

## Cost

A sample on card 1 took 14.7 ms of user time and 131 ms of system time
(the kernel generating about 1600 files), against 68 ms and 136 ms for the
Rust agent: `docs/results/2026-09-29-agent-assembly.md`.

## Testing

`host/crates/phi-rpc/tests/agent.rs`: `stat_from_a_fixture` builds a /proc
and /sys tree (text copied from the card on 2026-09-17, plus odd cases: a
gap in the CPU numbers, an unparsable core id, a capped load, a missing
channel, a negative temperature, a short diskstats line, a non-UTF-8 name, a
vanished pid) and compares six samples with what the rules above give,
including when kernel threads are and are not re-read;
`stat_without_hwmon_or_files` an empty tree; `stat_of_this_host` the
agent listing itself on the host.
