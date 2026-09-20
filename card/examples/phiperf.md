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

## What the counters are actually worth (2026-09-19)

Only `cycles` has been validated on this card. Treat the rest as unproven.

Measured with `xz -6 -T1` over 8 MB, two events so nothing multiplexes:

```
$ phiperf -n -r 0x2a,0x16 xz -6 -T1 -c /tmp/p8
raw 0x2a (cycles)         27557970377       100%
raw 0x16 (instructions)             0       100%
wall                           25.099 s
```

27.56e9 cycles at 1.1 GHz is 25.05 s against a 25.10 s wall, so cycles is
correct. Instructions reads zero in the same run, at full scaling with no
multiplexing to blame. `0x0016` is the code
`arch/x86/events/intel/knc.c` maps `PERF_COUNT_HW_INSTRUCTIONS` to, so
this is not a wrong constant here; the counter does not appear to work.

Worse, the default six-event set multiplexes onto two counters at 33% and
the scaled results are not merely imprecise but wrong: the same instruction
count came back as 15e9 in one run and 993,910 in another for comparable
work. A derived figure such as instructions per cycle is then pure noise,
and one was published from it and later retracted
(`docs/results/2026-09-19-xz.md`).

Rules of thumb until someone investigates:

- Pass `-n -r <two codes>` so the count is exact rather than scaled.
- Sanity-check any counter against something independent. Cycles divided by
  1.1 GHz should equal the wall clock for a single-threaded run; if it does
  not, the counter is not measuring what you think.
- Do not compute ratios between two events unless both have passed such a
  check.
- Multi-threaded runs are separately suspect: `zstd -T114` reported 861,817
  instructions for 20 MB of input, so the worker threads were not being
  followed at all.
