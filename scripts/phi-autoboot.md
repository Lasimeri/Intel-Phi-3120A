# phi-autoboot.sh

Boots the card at login. `install [DISK]` writes
`~/.config/systemd/user/phi.service`, a user unit wanted by
`default.target`, so the user's systemd instance starts it when the first
session opens and stops it when the last one closes (no lingering: the
card is reset by VFIO when the process ends, so "up while logged in" is
the honest description). The unit runs the same `phictl boot` as
`phi-up.sh`: control socket at `$XDG_RUNTIME_DIR/phictl/control.sock`
(so `phictl exec`, `phi-run.sh` and the other scripts work unchanged),
the SSH forwarder on 127.0.0.1:2222 and the persistent disk. Stopping
asks the card to power off through the socket first, so its filesystem
unmounts, then ends the process. `Restart=on-failure` with a 10 s delay
covers a boot that dies. `remove` disables and deletes the unit; `status`
shows it.

`phi-up.sh` refuses to start while the service's `phictl boot` runs
(its guard sees the process); use `systemctl --user stop phi.service` to
take the card over by hand, or `restart` to reboot it.

On this machine: `scripts/phi-autoboot.sh install /mnt/1TB-NVMe/phi/disk.img 4G`
(2026-09-16): the disk, and 4 GiB of host memory for swap and `/dev/phihost`
(`docs/spec/ring-protocol.md`, host memory). Requirements: group `phi` (`sudo scripts/setup-arch.sh`
once), the host tools built, the kernel and initramfs built, the disk
image created (`scripts/phi-disk.sh create`).
