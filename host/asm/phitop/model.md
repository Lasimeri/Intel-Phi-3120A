# model.S: from samples to what the screen shows

**Snapshots** are fixed records (`defs.md`): up to 4096 CPUs (core id,
busy and idle ticks), the six memory counters, three load averages,
running and total tasks, up to 64 temperatures, the peak, voltage and
clock, up to 16 block devices and 16 interfaces (name, bytes read and
written), up to 16384 processes (pid, state, kernel-thread flag,
threads, ticks, resident kB, name), the two process totals, the six
traffic counters, and the host's clock when the sample arrived. The
decoder walks the `StatReply` body in wire order (phi-rpc's
`put_stat`); the wire's lists beyond a capacity are skipped (the counts
sent are kept where the frame prints them).

**Derivation**, from the previous and the current snapshot, as the Rust
`derive` did it: the interval (host clock, at least 1 ms); each CPU's
load `busy / (busy + idle)` over the interval in units of 1/10000; the
cores (CPUs grouped by core id, cores in id order, threads in CPU order,
at most 8 per core); the mean load; DMA and aperture rates in bytes per
second; each device's read and write rates, sorted by name (a device
absent from the previous sample rates 0); each listed process's CPU
share in tenths of a percent of one hardware thread, from its ticks
since it was last reported over that span (USER_HZ 100): a kernel
thread the agent left out while idle gets its burst averaged over the
gap, a new process shows zero until its second report. The
co-processor worker's rates (`derive_vpu`, since 2026-10-09) come from
the two snapshots' copies of its stats line (the AVX-512 repository's
`card/vpu/vpu_proto.h`, `struct vpu_stats` at offset 320 of the card's
host-memory window, which `main.S` maps read-only): requests a second
and busy percent in tenths, each stage's time per request over the
interval, and the line's own counts; nothing is derived when either
sample lacks the line or a counter went backwards (a worker restarted
between the samples).

**History**: per slot, an open-addressing table of 65536 entries keyed
by pid (ticks, time reported); entries unseen for two minutes are
forgotten, with the probe chains after a removed entry reinserted.

Integers throughout: what the Rust code kept as `f64` is kept here in
the unit its display needs (loads in 1/10000, shares in tenths of a
percent, rates in bytes per second, intervals in microseconds).
