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
root. When SSH is wanted, boot with sudo as in `docs/howto/direct-access.md`
instead; the client commands find that daemon's socket automatically.

Companion scripts: `phi-down.sh` halts and releases the card,
`phi-run.sh CMD` runs a command on it.
