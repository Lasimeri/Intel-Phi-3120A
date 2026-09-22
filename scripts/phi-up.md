# phi-up.sh

Boots a card from an unprivileged shell and leaves it running in the
background with a control socket, so that `phictl exec`, `put`, `get` and
`status` (or `phi -c N run`) work without anyone at a console:

- `-c N` picks the card (else `$PHI_CARD`, else 0); `phictl cards` lists
  them. Every per-card path comes from `phi-env.sh`.
- No sudo. The udev rule from `setup-arch.sh` gives the VFIO group nodes
  to the `phi` group, and the socket lives in `$XDG_RUNTIME_DIR/phictl/`
  (card 0) or `$XDG_RUNTIME_DIR/phictl/N/` (mode 0600, owner-checked).
- The command line is assembled by `phi-boot.sh`, the same script the
  systemd unit runs, so a card booted here is identical to one booted at
  host boot: its socket, its SSH forward on `127.0.0.1:2222+N`, its ring
  subnet `10.9.N.0/24` and hostname `phiN`, its disk image and host memory
  from `~/.config/phi/cards`.
- `setsid nohup`: the boot process outlives the shell that started it (the
  card lives only while that process holds the VFIO device). Its pid is in
  the card's runtime directory as `boot.pid`, the console in `console.log`.
- Waits up to 90 s for the agent (about 15 s from a cold open: 9.5 s of
  GDDR training, 4 s of kernel), then prints the status line.
- `--toolchain` also loads the native clang (`clang-push.sh`, 25 s).
- `--disk PATH` overrides the image from `~/.config/phi/cards` (or
  `PHI_DISK` for card 0); `init` mounts it on `/data` and binds
  `/opt/phi`, `/root` and `/home` from it, so `--toolchain` is needed once
  per image, not per boot.
- `--host-mem SIZE` / `--no-host-mem` override the host RAM (default 6G).
- `--ssh` is accepted and ignored: every card gets its forward now.
- `PHI_CMDLINE_EXTRA` is appended to the kernel command line (for example
  `knc_blk.direct=0`); extra arguments after the options go to `phictl
  boot` (for example `--no-dma`).
- Refuses to start a card that is already up or whose daemon is running.

The network bridge (`--net`) is not started: creating a TAP device needs
root. The forward through the daemon's userspace stack
(`host/crates/phictl/src/forward.md`) is the way in without one:
`ssh -p 2222+N root@127.0.0.1`, or `phi -c N sh`.

Companion scripts: `phi-down.sh -c N` halts and releases the card,
`phi-run.sh -c N CMD` runs a command on it.

The running-daemon check matches the card's index or address in the
`phictl` command line: a plain `pgrep -f "phictl boot"` matched the shell
that ran the script when that shell's own command line mentioned the
pattern, and with several cards it would also match the other cards'
daemons.
