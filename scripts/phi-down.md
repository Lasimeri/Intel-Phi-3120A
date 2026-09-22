# phi-down.sh

Halts one card (`-c N`, else `$PHI_CARD`, else 0) and ends the `phictl
boot` process holding it, which resets the card when it closes the VFIO
device; removes the pid file and the socket. Safe to run when nothing is
up. `phi@N.service` runs it as `ExecStop`, where there is no pid file and
systemd ends the process itself afterwards.

The card gets a plain `poweroff` through the agent, not `poweroff -f`.
Plain `poweroff` signals PID 1, so `init` can stop the services, release
swap and unmount `/data` before the kernel halts (POST "KH"); `-f` calls
`reboot(2)` from the calling shell and never reaches init, which left the
ext4 journal to replay on every boot until 2026-09-19
(`docs/results/2026-09-19-card-os.md`).

After sending it, the script polls `phictl status` until the agent stops
answering, up to 8 s. `init` stops the agent last, after the unmount, so
that is the signal that the card is finished with the disk; ending `phictl`
any earlier takes the block service away mid-unmount.
