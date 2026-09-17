# phitop: the card's resources, live, from the host

```
host/target/debug/phitop [--interval 1.0] [--kthreads] [--socket PATH]
host/target/debug/phitop --batch 3          # plain text frames, for scripts and logs
```

A full-screen viewer in the spirit of glances, but for the card and run
on the host: nothing is installed on the card beyond the agent it
already runs. Each interval it makes two short requests on the daemon's
control socket: `Stat`, which the daemon relays to the card and the
agent answers from /proc and sysfs (`card/agent/src/stat.md`), and
`Traffic`, which the daemon answers from its own PCIe byte counters
(`phi-vfio/src/traffic.md`). The frame (`view.md`) shows:

- every hardware thread's load on a grid of 57 cores by 4 threads, and
  the mean per core;
- the nine die sensors, the hottest now, the peak since boot, board
  sensors when present;
- memory and page cache, swap on host RAM (`/dev/phiblk1`);
- PCIe traffic to and from the card, split into DMA engine and aperture;
- the card's block device and interface rates;
- the card's processes with CPU share, resident size and thread count;
  kernel threads only when they used CPU time (`k` shows or hides them).

Keys: `q` quit, `c` and `m` sort by CPU or memory, `k` kernel threads,
`+` and `-` the interval (0.25 to 60 s). Ctrl-C and SIGTERM restore the
terminal too.

The viewer stays live while another client runs a command through
`phictl exec`: the daemon interleaves `Stat` with the session and the
agent answers it from inside a running command (`phictl/src/serve.md`).
If the daemon is not running or the card is down, the first line shows
the error in red and the viewer keeps trying at the same interval.

Load is computed from tick differences, so the first frame appears after
two samples (one interval). Rates use the host clock between arrivals.
