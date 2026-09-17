# stat.rs: the card's state as one sample

Answers the host's `Stat` request (`phi-rpc`) with everything `phitop`
shows, read from the card's own /proc and sysfs:

| field | source |
| --- | --- |
| per-CPU busy and idle ticks, core id | `/proc/stat` cpuN lines; `/sys/devices/system/cpu/cpuN/topology/core_id` (read once) |
| memory and swap | `/proc/meminfo` |
| load averages, runnable and total tasks | `/proc/loadavg` |
| temperatures, peak die temperature, core voltage and clock | the `knc` hwmon (kernel patch 0027): `temp1..15_input`, `temp1..9_max`, `in0_input`, `core_mhz` |
| bytes read and written per block device | `/proc/diskstats`, devices named `phiblk*` (sectors times 512) |
| bytes received and sent per interface | `/proc/net/dev`, every interface but `lo` |
| processes | `/proc/[pid]/stat`: state, PF_KTHREAD flag, utime plus stime, thread count, command name; `/proc/[pid]/status` VmRSS for user processes (the stat line's rss field reads 0 for small processes on 228 CPUs: it is the global part of a per-CPU counter whose batch is 456 pages) |

Counters are sent as they are; the host differences two samples for
rates and shares (`host/crates/phitop/src/model.md`).

Kernel threads: the card runs about 1500 of them (six or seven per CPU),
almost all asleep. Sending them every second would cost 60 KiB per
sample, and reading their 1500 stat files cost a sixth of a hardware
thread (measured 2026-09-17: the agent at 17 percent in its own table),
so the sampler keeps their tick counts, re-reads known kernel threads
only every fourth sample, and sends a kernel thread only when its count
moved since it was last read. User processes and new pids are read and
sent every sample. The totals `nprocs` and `nkthreads` count everything.

The command name is taken between the first "(" and the last ")" of the
stat line, as proc(5) requires, so a name containing spaces or
parentheses parses. Tested with the line of the card's init.
