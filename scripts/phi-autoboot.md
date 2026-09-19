# phi-autoboot.sh

Runs the card from a systemd user service. `install [DISK [HOSTMEM]]`
writes `~/.config/systemd/user/phi.service`, a user unit wanted by
`default.target`. When it starts depends on one setting:

| Mode | Command | Lifetime |
| --- | --- | --- |
| At login (default) | `phi-autoboot.sh at-login` | Starts when the first session opens, stops when the last one closes. The card is reset by VFIO when the process ends, so "up while logged in" is the honest description |
| At host boot | `phi-autoboot.sh at-boot` | Starts during boot with nobody logged in, survives logout, and keeps running at the lock screen and the greeter |

`at-boot` works by turning on lingering (`loginctl enable-linger`), which
makes `systemd-logind` start this user's systemd instance at host boot
instead of at the first login. That is what lets a *user* unit behave like
a boot service without granting it root, which keeps ADR 0001's model:
the daemon runs as the owning user, the socket stays at
`$XDG_RUNTIME_DIR/phictl/control.sock` mode 0600, and the card is still
reachable only through that socket or the loopback SSH forwarder.

**Lingering is per user, not per unit.** Every enabled user unit starts at
boot as well, not only this one. `at-boot` prints the list so the cost is
visible; on this machine it is wireplumber, pipewire and its two sockets,
p11-kit-server, xdg-user-dirs and two timers. If one of those should not
run headless, disable that unit rather than lingering.

`at-login` reverses it. `remove` deletes the unit and deliberately leaves
lingering alone, since it is a property of the user account rather than of
this project.

The unit runs the same `phictl boot` as
`phi-up.sh`: control socket at `$XDG_RUNTIME_DIR/phictl/control.sock`
(so `phictl exec`, `phi-run.sh` and the other scripts work unchanged),
the SSH forwarder on 127.0.0.1:2222 and the persistent disk.

Stopping sends a plain `poweroff` through the socket and waits for the agent
to stop answering, which is how it knows `init` finished unmounting `/data`,
then ends the process (`scripts/phi-down.md` explains why plain and not
`poweroff -f`). `Restart=on-failure` with a 10 s delay covers a boot that
dies. `remove` disables and deletes the unit; `status`
shows it.

`phi-up.sh` refuses to start while the service's `phictl boot` runs
(its guard sees the process); use `systemctl --user stop phi.service` to
take the card over by hand, or `restart` to reboot it.

On this machine: `scripts/phi-autoboot.sh install /mnt/1TB-NVMe/phi/disk.img 6G`
(2026-09-19, was 4G from 2026-09-16): the disk, and 6 GiB of host memory
for swap and `/dev/phihost`
(`docs/spec/ring-protocol.md`, host memory). Requirements: group `phi` (`sudo scripts/setup-arch.sh`
once), the host tools built, the kernel and initramfs built, the disk
image created (`scripts/phi-disk.sh create`).

## When the unit will not come up

`Restart=on-failure` with a 10 s delay means a permanent fault shows as
`activating (auto-restart)` rather than `failed`; `systemctl --user status
phi.service` is the only place the reason appears. The two that have been
seen:

| Status line | Cause | Fix |
| --- | --- | --- |
| `is not bound to vfio-pci` | The card came up with no driver after a host reboot | `sudo scripts/bind-vfio.sh` now; `sudo scripts/setup-arch.sh` so it sticks (`scripts/setup-arch.md`) |
| `No such file or directory` on the kernel or initramfs path | The build tree under `~/.cache` was cleared, or the symlinks point at a different user | Rebuild: `card/kernel/build.sh all`, `card/initramfs/build.sh` |

The unit holds the card for as long as the session lasts, so a failing
unit that keeps restarting also keeps resetting the card through VFIO.
Stop it (`systemctl --user stop phi.service`) before diagnosing by hand.
