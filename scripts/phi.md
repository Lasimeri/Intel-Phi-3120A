# phi.sh

One command for everything a person does with the card from this host.
`phi.sh install-cli` symlinks it as `~/.local/bin/phi` and symlinks the fish
completions in `phi.fish` into `~/.config/fish/completions/`, so `phi <tab>`
lists the subcommands and an edit in the repository takes effect at once.

Everything it does works from an unprivileged shell in the `phi` group.
Nothing here needs `sudo`.

| Command | What it does |
| --- | --- |
| `phi up [ARGS]` | Starts the card. With the systemd user unit installed and no arguments, starts the unit and waits for the agent; with arguments, or without the unit, runs `scripts/phi-up.sh` instead and passes them to `phictl boot` |
| `phi down` | Stops the unit, or runs `scripts/phi-down.sh`. Either way the card powers off through init, so its disk unmounts |
| `phi restart` | Down, then up |
| `phi status` | Unit state, socket, agent, kernel, CPU count, memory, swap, disk, and the list of paths that can reach the card |
| `phi run CMD` | Runs a command through the control socket. Stdin, stdout, stderr and the exit status are relayed |
| `phi sh [CMD]` | An interactive login shell on the card over the loopback SSH forward, or one command in a login shell |
| `phi put`, `phi get` | Copy a file in or out through the control socket |
| `phi top` | `phitop`, the live viewer |
| `phi sensors`, `phi traffic` | Answered by the daemon itself, so they work while the card's kernel is booting, hung or halted. They do need the daemon running: it is the process holding the VFIO device |
| `phi console` | Follows the card's console: the log file for an unprivileged boot, the journal for the unit |
| `phi log` | The unit's journal, or the console log |
| `phi disk ARGS` | `scripts/phi-disk.sh` (create, check, usage) |
| `phi install-cli` | The symlink and the completions |

## Finding the control socket

The friction this removes: `phictl` needs `PHICTL_SOCKET` or a `--socket`
flag whose position within the command line matters, and the path differs
between an unprivileged boot and a root one. `phi` resolves it once, in this
order, taking the first that is a live socket:

1. `$PHICTL_SOCKET`
2. `$XDG_RUNTIME_DIR/phictl/control.sock`
3. `/run/user/<uid>/phictl/control.sock`
4. `/run/phictl/control.sock` (a root boot)

If none exists it still names the second one, so the error message points
somewhere useful rather than at an empty variable.

## `phi run` against `phi sh`

Two doors, and they behave differently on purpose.

`phi run` goes through the control socket to `phi-agent` on the card. No
SSH, no network, no pty: it relays bytes and an exit status. Use it for
anything scripted. The agent sets the card's `PATH` itself, so
`/opt/phi/bin` is present.

`phi sh` goes over SSH through the daemon's userspace TCP forwarder on
`127.0.0.1:2222`. It gets a real pty, so a full-screen program, job control
and Ctrl-C work. With no arguments it is an interactive login shell, which
reads `/etc/profile` on the card. With arguments, `phi` wraps them in
`sh -lc` and single-quotes them, because `ssh host command` otherwise runs a
non-login shell whose `PATH` lacks `/opt/phi/bin`; the quoting means `$VAR`
in the command is expanded on the card, not on the host.

## What `phi status` says about reach

The access surface is part of the status output because it is the thing
worth keeping an eye on:

- the control socket, mode 0600 in a 0700 directory, checked with
  `SO_PEERCRED` on every connection, so only the owning user and root;
- `127.0.0.1:2222` when the daemon runs the forwarder, which binds loopback
  and nothing else;
- a warning line if anything is listening on port 2222 beyond loopback;
- a line naming a host TAP device if one exists, because that is the one
  configuration in which the card is reachable from outside this machine
  (a root boot with `--net`, which nothing here does by default).

## Why a wrapper and not aliases

The subcommands are not one-to-one with `phictl`: `up` chooses between the
systemd unit and the script, `down` has to reach the card through the socket
before it may end the process holding it, `sh` has to check the forwarder is
there before invoking ssh, and `status` merges four sources. A shell alias
per command cannot do any of that, and a person should not have to know
which of `phictl`, `phitop`, `phi-up.sh`, `phi-down.sh` and `systemctl
--user` owns a given verb.
