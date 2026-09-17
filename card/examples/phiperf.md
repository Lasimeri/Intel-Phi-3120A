# phiperf.c

A `perf stat` for the card, since the perf tool itself is not built for
it. The kernel's Knights Corner PMU driver (`arch/x86/events/intel/knc.c`,
"Performance Events: knc PMU driver" at boot) maps the six generic
hardware events to the core's counters (cycles 0x2a, instructions 0x16,
cache references 0x28 and misses 0x29, branches 0x12, mispredictions
0x2b; two 40-bit counters per hardware thread), and `perf_event_open`
counts them inherited across the command's threads and children. With
more events than counters the kernel multiplexes; the tool scales counts
by the time each event ran and prints that share.

```
cc -O2 -o phiperf phiperf.c
./phiperf ./mandel out.png 1920 1080 2000 228
./phiperf -n -r 0x10cb,0x10cc ./mandel-vpu out.png 7680 4320 2000 228   # L2 read misses, L2 write hits only: exact
./phiperf -r 0x0003 ./mandel ...    # generic six plus one raw, all multiplexed estimates
```

Raw event codes are the KNC PMU's (Intel's "Xeon Phi Coprocessor
Performance Monitoring Units" list); the kernel's cache map in `knc.c`
has a few: data read 0x0000/miss 0x0003, data write 0x0001/miss 0x0004,
code read 0x000c/miss 0x000e, L2 read miss 0x10cb, L2 write hit 0x10cc,
L2 prefetch 0x10fc/miss 0x10fe.

Results: `docs/results/2026-09-16-sensors.md`.
