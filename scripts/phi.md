# phi.sh

One command for everything a person does with the cards from this host.
`phi.sh install-cli` symlinks it as `~/.local/bin/phi` and symlinks the fish
completions in `phi.fish` into `~/.config/fish/completions/`, so `phi <tab>`
lists the subcommands and an edit in the repository takes effect at once.

It also symlinks the binaries themselves: `phitop`, `phictl`,
`phi-isa-audit` and `knc-mvex-gen`. A wrapper that hides its own tools is
the reason `phitop` reported "not found" while `phi top` worked, which is
the opposite of what a wrapper is for. Both spellings work now, and
`phitop` finds the control socket on its own.

Everything it does works from an unprivileged shell in the `phi` group.
Nothing here needs `sudo`.

## Which card

```
phi [-c N] COMMAND ...
```

`-c N` (or `--card N`, or `$PHI_CARD`) names a card by its index, 0 to
15; without it, card 0. `phi cards` lists them:

```
2 card(s) known, order from /home/lasimeri/.config/phi/cards
card  bdf           present link         driver    group  up    hostmem  disk
0     0000:2f:00.0  yes     5.0 x8       vfio-pci  34     yes   6G       /mnt/1TB-NVMe/phi/disk.img
1     0000:24:00.0  yes     5.0 x4       vfio-pci  27     yes   6G       /mnt/1TB-NVMe/phi/disk1.img
```

The index is the whole identity: it selects the PCI device, the control
socket, the SSH port (2222 + N), the ring subnet, the hostname (`phiN`),
the disk image and host memory, and the systemd instance `phi@N`
(`host/crates/phi-vfio/src/cards.md` has the table). `phi cards init`
writes `~/.config/phi/cards` from the bus so the order is pinned; without
that file the cards are ordered by PCI address, which changed underneath
the stack the day the second card arrived.

| Command | What it does |
| --- | --- |
| `phi cards` | Every card: index, address, link, driver, group, whether a daemon answers, disk, host memory; and the unit states |
| `phi up [ARGS]` | Starts the card (`phi up all`: every card). With the systemd unit installed and no arguments, starts `phi@N` and waits for the agent; with arguments, or without the unit, runs `scripts/phi-up.sh -c N` instead and passes them on |
| `phi down` | Stops the unit, or runs `scripts/phi-down.sh -c N` (`phi down all`: every card). Either way the card powers off through init, so its disk unmounts |
| `phi restart` | Down, then up |
| `phi status` | With no card named and several present, the cards table and one line per card; otherwise card, unit state, socket, agent, hostname, kernel, CPU count, memory, swap, disk, and the list of paths that can reach the card |
| `phi run CMD` | Runs a command through the control socket. Stdin, stdout, stderr and the exit status are relayed |
| `phi sh [CMD]` | An interactive login shell on the card over its loopback SSH forward, or one command in a login shell |
| `phi put`, `phi get` | Copy a file in or out through the control socket |
| `phi top` | `phitop`, the live viewer, on this card's socket. `phitop -c N` on its own works too |
| `phi sensors`, `phi traffic` | Answered by the daemon itself, so they work while the card's kernel is booting, hung or halted. They do need the daemon running: it is the process holding the VFIO device |
| `phi console` | Follows the card's console: the log file for an unprivileged boot, the journal for the unit |
| `phi log` | The unit's journal, or the console log |
| `phi disk ARGS` | `scripts/phi-disk.sh` (create, check, usage) |
| `phi vpu ARGS` | The AVX-512 co-processor worker on this card, from its own repository, found as the family finds a sibling (`PHI_AVX512_ROOT`, else a checkout next to this one, else in `$HOME`, as `Intel-Phi-AVX512` or `Intel Phi AVX-512`): `deploy`, `start`, `stop`, `status`, `log`, `poly` |
| `phi ssh-config [--apply]` | One `~/.ssh/config` stanza per known card (`ssh phi`, `ssh phi1`, ...), each on its own forward port with the one pinned host key; printed, or appended with `--apply` |
| `phi install-cli` | The symlink and the completions |

## Finding the control socket

The friction this removes: `phictl` needs `PHICTL_SOCKET` or a `--socket`
flag whose position within the command line matters, and the path differs
between an unprivileged boot and a root one and between cards. `phi`
resolves it once, in this order, taking the first that is a live socket:

1. `$PHICTL_SOCKET`
2. this card's socket in `$XDG_RUNTIME_DIR` (`phictl/control.sock` for
   card 0, `phictl/N/control.sock` for card N)
3. the same under `/run/user/<uid>`
4. `/run/phictl[/N]/control.sock` (a root boot)

If none exists it still names the second one, so the error message points
somewhere useful rather than at an empty variable.

## `phi run` against `phi sh`

Two doors, and they behave differently on purpose.

`phi run` goes through the control socket to `phi-agent` on the card. No
SSH, no network, no pty: it relays bytes and an exit status. Use it for
anything scripted. The agent sets the card's `PATH` itself, so
`/opt/phi/bin` is present.

`phi sh` goes over SSH through the daemon's userspace TCP forwarder on
`127.0.0.1:2222+N`. It gets a real pty, so a full-screen program, job
control and Ctrl-C work. With no arguments it is an interactive login
shell, which reads `/etc/profile` on the card. With arguments, `phi` wraps
them in `sh -lc` and single-quotes them, because `ssh host command`
otherwise runs a non-login shell whose `PATH` lacks `/opt/phi/bin`; the
quoting means `$VAR` in the command is expanded on the card, not on the
host.

Every card boots the same image and therefore presents the same SSH host
key, so `phi sh` passes `HostKeyAlias=phi`: one pinned entry in
`~/.ssh/known_hosts_phi` covers every port. A card with a different key
(a different image) is refused, which is the right answer.

## What `phi status` says about reach

The access surface is part of the status output because it is the thing
worth keeping an eye on. Each line names the **command**, not just the
address:

```
ways in (all of them local to this host):
  phi -c 1 run CMD       control socket /run/user/1000/phictl/1/control.sock
                         mode 0600 in a 0700 dir, SO_PEERCRED per connection
  phi -c 1 sh            SSH with a pty, through the forwarder on 127.0.0.1:2223
  ssh phi1               the same, through your ~/.ssh/config stanza
```

It used to print a bare `127.0.0.1:2222`, which reads like something you
can hand to ssh. It is not: ssh takes `-p PORT` and a plain hostname, never
`host:port` (that is scp and rsync syntax), so `ssh root@127.0.0.1:2222`
fails with "Could not resolve hostname". The third line appears only when a
`Host ... phiN ...` stanza exists in `~/.ssh/config`; without one the status
prints the `-p` form instead, so the output is always a command that works
as shown.

Two more lines appear when they apply:

- a warning if anything is listening on that card's port beyond loopback;
- a host TAP device if one exists, because that is the one configuration in
  which a card is reachable from outside this machine (a root boot with
  `--net`, which nothing here does by default).

The `starts` line reports whether the unit comes up at host boot (lingering
on) or at the first login, which is the difference between a card that is
there at the greeter and one that waits for you.

## Why a wrapper and not aliases

The subcommands are not one-to-one with `phictl`: `up` chooses between the
systemd unit and the script, `down` has to reach the card through the socket
before it may end the process holding it, `sh` has to check the forwarder is
there before invoking ssh, and `status` merges four sources. A shell alias
per command cannot do any of that, and a person should not have to know
which of `phictl`, `phitop`, `phi-up.sh`, `phi-down.sh` and `systemctl
--user` owns a given verb.
