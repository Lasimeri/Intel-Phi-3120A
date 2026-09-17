# Monitoring the card

| what | where | command |
| --- | --- | --- |
| temperatures, voltage, clock, from the host | daemon reads the SBOX | `host/target/debug/phictl sensors` |
| the same on the card | hwmon `knc` (patch 0027) | `cat /sys/class/hwmon/hwmon0/temp1_input` (millidegrees), `..._label`, `in0_input` (mV), `core_mhz` |
| load per CPU, memory | busybox, htop, glances | `htop`, `glances` (htop-phi, glances-phi repos, pushed onto the disk) |
| hardware counters around a command | `phiperf` (`card/examples/phiperf.c`) | `./phiperf ./program args`, `./phiperf -n -r 0x10cb,0x10cc ./program` |
| disk and host-memory traffic | the host tool's console | `[phictl] disk:` lines in `$XDG_RUNTIME_DIR/phictl/console.log` |
| service state | systemd | `systemctl --user status phi.service`, `journalctl --user -u phi.service` |

Absent on this card: board temperatures and power (SMC telemetry not
requested by this port), a fan (passive card). Results and numbers:
`docs/results/2026-09-16-sensors.md`.
