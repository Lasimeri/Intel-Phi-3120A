# phi-up.sh

Boots the card from an unprivileged shell and leaves it running in the
background with a control socket, so that `phictl exec`, `put`, `get` and
`status` (or `scripts/phi-run.sh`) work without anyone at a console:

- No sudo. The udev rule from `setup-arch.sh` gives the VFIO group node to
  the `phi` group, and `phictl boot --serve PATH` puts the socket in
  `$XDG_RUNTIME_DIR/phictl/` (mode 0600, owner-checked).
- `setsid nohup`: the boot process outlives the shell that started it (the
  card lives only while that process holds the VFIO device). Its pid is in
  `$XDG_RUNTIME_DIR/phictl/boot.pid`, the console in `console.log`.
- Waits up to 90 s for the agent (about 15 s from a cold open: 9.5 s of
  GDDR training, 4 s of kernel), then prints the status line.
- `--toolchain` also loads the native clang (`clang-push.sh`, 25 s).
- Refuses to start when another `phictl boot` holds the card.

The network bridge (`--net`) is not started: creating a TAP device needs
root. For SSH, `--ssh` forwards a host port through a userspace stack
instead (`ssh -p 2222 root@localhost`); a sudo boot with `--net` remains
the way to give the card a real interface on the host.

Companion scripts: `phi-down.sh` halts and releases the card,
`phi-run.sh CMD` runs a command on it.

The running-boot check matches command lines that start with the phictl
binary (optionally behind `sudo`): a plain `pgrep -f "phictl boot"` also
matched the shell that ran the script when that shell's own command line
mentioned the pattern.

`--ssh` (2026-09-14) adds `--forward 2222:22`: phictl runs a userspace
network stack on the ring (`host/crates/phictl/src/forward.md`) and
forwards `127.0.0.1:2222` to the card's dropbear, so
`ssh -p 2222 root@localhost` works from an unprivileged boot; the TAP
bridge is no longer the only way in.
