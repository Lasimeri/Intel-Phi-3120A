# phi-autoboot.sh

Runs the cards from systemd user services: one template unit,
`~/.config/systemd/user/phi@.service`, whose instance number is the card
index (`phictl cards`). `install` writes the template and enables
`phi@N` for every card on the bus (or the one named); each instance runs
`scripts/phi-boot.sh -c N`, which is the same command line `phi-up.sh`
uses, so a card booted by hand and a card booted at host boot are the
same card. When they start depends on one setting:

| Mode | Command | Lifetime |
| --- | --- | --- |
| At login (default) | `phi-autoboot.sh at-login` | Start when the first session opens, stop when the last one closes. The cards are reset by VFIO when their processes end, so "up while logged in" is the honest description |
| At host boot | `phi-autoboot.sh at-boot` | Start during boot with nobody logged in, survive logout, and keep running at the lock screen and the greeter |

`at-boot` works by turning on lingering (`loginctl enable-linger`), which
makes `systemd-logind` start this user's systemd instance at host boot
instead of at the first login. That is what lets a *user* unit behave like
a boot service without granting it root, which keeps ADR 0001's model:
each daemon runs as the owning user, each socket stays in
`$XDG_RUNTIME_DIR/phictl` mode 0600, and each card is reachable only
through its socket or its loopback SSH forwarder.

**Lingering is per user, not per unit.** Every enabled user unit starts at
boot as well, not only these. `at-boot` prints the list so the cost is
visible; on this machine it is wireplumber, pipewire and its two sockets,
p11-kit-server, xdg-user-dirs and two timers. If one of those should not
run headless, disable that unit rather than lingering.

`at-login` reverses it. `remove [N|all]` disables an instance, or all of
them and the template, and deliberately leaves lingering alone, since it
is a property of the user account rather than of this project.

## What each instance gets

From `phi-env.sh`, by index: control socket
`$XDG_RUNTIME_DIR/phictl/control.sock` for card 0 and
`.../phictl/N/control.sock` for card N (so `phictl exec`, `phi -c N run`
and the other scripts work unchanged), the SSH forwarder on
`127.0.0.1:2222+N`, the ring subnet `10.9.N.0/24`, hostname `phiN`, and
the disk image and host memory from that card's line in
`~/.config/phi/cards` (`phi cards init` writes it; card 0 also honours
`PHI_DISK` and `PHI_HOST_MEM`).

Stopping runs `phi-down.sh -c N`: a plain `poweroff` through the socket,
then a wait for the agent to stop answering, which is how it knows `init`
finished unmounting `/data`; systemd then ends the process
(`scripts/phi-down.md` explains why plain and not `poweroff -f`).
`Restart=on-failure` with a 10 s delay covers a boot that dies.

`phi-up.sh` refuses to start a card whose daemon is already running; use
`phi -c N down` (which stops the unit) to take a card over by hand, or
`phi -c N restart` to reboot it.

## Migration from the single-card unit

`install` removes `phi.service`, the unit the single-card stack wrote,
after stopping it, and `phi@0.service` takes its place with the same
arguments. Nothing else changes for card 0: same socket, same port, same
disk, same host-memory file.

On this machine (2026-09-22): `scripts/phi-autoboot.sh install all` with
`~/.config/phi/cards` naming card 0 as `0000:2f:00.0` (the 3120A on the
CPU slot, `disk.img`) and card 1 as `0000:24:00.0` (the chipset card,
`disk1.img`), 6 GiB of host memory each. Requirements: group `phi`
(`sudo scripts/setup-arch.sh` once), the host tools built, the kernel and
initramfs built, a disk image per card (`scripts/phi-disk.sh create`).

## When a unit will not come up

`Restart=on-failure` with a 10 s delay means a permanent fault shows as
`activating (auto-restart)` rather than `failed`; `systemctl --user status
phi@N.service` is the only place the reason appears. The ones that have
been seen:

| Status line | Cause | Fix |
| --- | --- | --- |
| `is not bound to vfio-pci` | The card came up with no driver after a host reboot | `sudo scripts/bind-vfio.sh` now; `sudo scripts/setup-arch.sh` so it sticks (`scripts/setup-arch.md`) |
| `No such file or directory` on the kernel or initramfs path | The build tree under `~/.cache` was cleared, or the symlinks point at a different user | Rebuild: `card/kernel/build.sh all`, `card/initramfs/build.sh` |
| `card N is not on the PCI bus` | The card in `~/.config/phi/cards` at that index did not enumerate | `phictl cards`; reseat or repower it (the 300 W SKU refuses to power up without both auxiliary connectors) |
| `disk image ... not found` | No image for that card yet | `scripts/phi-disk.sh create PATH SIZE` and name it in `~/.config/phi/cards` |

A unit holds its card for as long as the session lasts, so a failing
unit that keeps restarting also keeps resetting that card through VFIO.
Stop it (`systemctl --user stop phi@N.service`) before diagnosing by hand.
