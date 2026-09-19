# Monitoring the card

Verified against the live card on 2026-09-17; the numbers are in
`docs/results/2026-09-16-sensors.md` and `2026-09-17-phitop.md`.

| what | where | command |
| --- | --- | --- |
| the card live from the host: every hardware thread's load on a 57 by 4 grid, die temperatures, memory and swap, PCIe rates by path and direction, the card's disk and network rates, processes | `phitop` (host, `host/crates/phitop`), fed by the agent's `Stat` sideband and the daemon's byte counters | `phi top` (full screen; `phi top -i 3` for a 3 s interval); `phi top --batch 3` prints three plain-text frames and exits (logs, scripts) |
| temperatures, voltage, clock, from the host without the card's help | the daemon reads the SBOX through the MMIO BAR, so this answers while the card boots or hangs | `phi sensors` |
| the same on the card | hwmon `knc` (kernel patch 0027); `hwmon0` on this card, find it with `grep -l knc /sys/class/hwmon/hwmon*/name` | `cat /sys/class/hwmon/hwmon0/temp1_input` (millidegrees), `temp1_label` (`die0`), `temp1_max` (the SBOX's recorded maximum), `temp10..15` board sensors (present in name only: the SMC is not asked), `in0_input` (core voltage, mV), `core_mhz`, `core_ratio_mhz`, `raw` |
| load per CPU, memory, processes, on the card | busybox `top`, or htop and glances from the sibling repositories htop-phi and glances-phi, installed on the disk image | `phi sh` then `top` or `htop`, or `phi run top -b -n1` |
| bytes the daemon moved over PCIe since it started | `phi_vfio::traffic` counters at the two copy paths (aperture, DMA engine) | `phi traffic` |
| hardware counters around a command | `phiperf` (`card/examples/phiperf.c`, compiled on the card), a `perf stat` for the KNC PMU driver (two counters per core) | `./phiperf ./program args`; `./phiperf -n -r 0x10cb,0x10cc ./program` for raw events only |
| disk and host-memory service messages | the daemon's console | `[phictl] disk:` and `[phictl] host memory:` lines in `$XDG_RUNTIME_DIR/phictl/console.log` (`phi-up.sh`) or `journalctl --user -u phi.service` (autoboot) |
| service state | systemd user instance | `phi status`, `phi log -f`, or `systemctl --user status phi.service` |

`phitop` keys: `q` quit, `c` and `m` sort processes by CPU or memory, `k`
show or hide the kernel threads the card reports (those that used CPU
time), `+` and `-` change the interval by 0.5 s (0.25 to 60 s); Ctrl-C and
SIGTERM restore the terminal. Options: `--interval SECONDS`, `--batch N`,
`--kthreads`, `--socket PATH` (`phitop --help`). Load is computed from tick
differences, so the first frame appears one interval after start. The
viewer stays live while a `phictl exec` session runs (ADR 0009); if the
daemon is down the first line shows the error and it keeps retrying.

What the card does not report: board temperatures and power (the SMC
broadcasts them only to a driver that asks over I2C, which this port has
none of) and fan speed (same path; Intel lists the 3120A as an actively
cooled SKU, `docs/hardware.md` on what is known about this unit's cooling).
The die sensors, core voltage and clock come straight from SBOX registers.
