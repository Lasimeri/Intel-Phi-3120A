# The card runs from host boot, with nobody logged in

Host 7.2.6-1-cachyos. Change: `phi.service` now starts when the host boots
rather than when a session opens, so the card is up at the greeter, at the
lock screen and after the last logout. Reachability is unchanged; only the
duration is.

## The mechanism, and why it is not a system unit

`phi.service` is a **user** unit, and ADR 0001's whole point is that the
host side is an unprivileged userspace program. Moving it to a system unit
would have meant either running the daemon as root or inventing a service
account, plus a socket path outside `$XDG_RUNTIME_DIR`.

Lingering gets the same result without touching any of that:

```
$ loginctl enable-linger lasimeri     # polkit allowed this without sudo
$ loginctl show-user lasimeri -p Linger
Linger=yes
```

`systemd-logind` then starts this user's systemd instance at host boot
instead of at the first login, and keeps it after the last logout. The unit
is already `WantedBy=default.target`, so nothing about it had to change to
be started that way. The daemon still runs as `lasimeri`, the socket is
still `/run/user/1000/phictl/control.sock` mode 0600 in a 0700 directory,
and `SO_PEERCRED` still gates every connection.

`scripts/phi-autoboot.sh at-boot` and `at-login` set and clear it.

## The cost, which is not optional

Lingering is a property of the **user**, not of a unit. Every enabled user
unit starts at boot with it, so `at-boot` prints the full list rather than
leaving it implicit. On this machine:

```
phi.service  wireplumber.service  xdg-user-dirs.service
p11-kit-server.socket  pipewire-pulse.socket  pipewire.socket
arch-update.timer  claude-md-date.timer
```

There is no way to scope lingering to one unit. Anything in that list that
should not run headless has to be disabled individually.

## The race this opened, and the guard

At login the VFIO device has existed for minutes. At boot it has not:
`systemd-modules-load` inserts `vfio_pci`, the `ids=8086:225d` option makes
it claim the card, and udev applies the group-`phi` ownership, all
asynchronously, and a user unit cannot be ordered against system units.

Measured on the 03:23 boot: `/dev/vfio/30` appeared at 03:23:27.68, while
the user manager (started by login, not by lingering) came up at 03:27:25.
That four-minute gap is an artifact of the login, not a bound on the race,
so it proves nothing about the lingering case. The guard is written as if
the race is real.

The unit used to carry:

```
ConditionPathExists=/dev/vfio/vfio
```

which is wrong twice over. A failed condition marks a unit **skipped**, not
failed, and `Restart=on-failure` never acts on a skip, so a card that was
not ready would leave a unit that reads like success and did nothing. And
`/dev/vfio/vfio` is the container node: it says the `vfio` module loaded,
not that this card is bound or that its group node is ours.

It is replaced by `scripts/phi-wait-vfio.sh`, an `ExecStartPre` that polls
for the end state (bound to `vfio-pci`, has an IOMMU group,
`/dev/vfio/<group>` writable) and exits non-zero after a timeout, which
`Restart=on-failure` does act on. It is a script rather than an inline
snippet because systemd expands `$` in `Exec` lines itself.

Verified directly:

```
$ scripts/phi-wait-vfio.sh 5
(exit 0, 0.01 s, card ready)

$ PHI_BDF=0000:00:03.1 scripts/phi-wait-vfio.sh 3
phi-wait-vfio.sh: 0000:00:03.1 is bound to 'pcieport' after 3s, not vfio-pci.
  sudo scripts/bind-vfio.sh binds it now; sudo scripts/setup-arch.sh makes it stick.
(exit 1)

$ PHI_BDF=0000:ff:ff.0 scripts/phi-wait-vfio.sh 2
phi-wait-vfio.sh: 0000:ff:ff.0 is not a PCI device on this host
(exit 1)
```

The unit path needed quoting, which is worth recording because it failed
silently in the obvious way: the repository lives at `~/Intel Phi 3120A`,
systemd splits `Exec` lines on whitespace, and an unquoted
`ExecStartPre=/home/lasimeri/Intel Phi 3120A/scripts/...` parses as the
executable `/home/lasimeri/Intel` with three arguments. The generator emits
`ExecStartPre="$wait" 60`, matching what it already did for `ExecStart`.

After a restart, systemd reports the pre-exec ran:

```
Process: 3791 ExecStartPre=/home/lasimeri/Intel Phi 3120A/scripts/phi-wait-vfio.sh 60 (code=exited, status=0/SUCCESS)
Main PID: 3806 (phictl)
```

## What is verified and what is not

| Claim | Status |
| --- | --- |
| `Linger=yes` is set and survives `at-login`/`at-boot` round trips | Verified |
| The wait script returns 0 on a ready card and 1 with a specific message on each failure | Verified, all three paths |
| The unit starts with the quoted `ExecStartPre` and reaches a running card | Verified by restart |
| `phi status` reports which mode is in effect | Verified |
| The unit actually starts during boot with no session | **Not verified.** It needs a reboot |

The check that settles the last row, run after the next boot and before
logging in (over SSH from elsewhere, or by looking at the greeter):

```
loginctl list-sessions      # empty, or no session for this user
phi status                  # unit active, agent reachable
```

Stated plainly because this session has already produced two claims that
looked verified and were not: the `vfio-pci` `ids` option's sysfs file that
never exists, and `phi sensors` working "while the card is down".
