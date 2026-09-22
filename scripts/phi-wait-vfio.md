# phi-wait-vfio.sh

Blocks until a card's VFIO group node exists and this process can open
it, or fails after a timeout (default 60 s). `phi@N.service` runs it as
`ExecStartPre` with `-c N`; `PHI_BDF` overrides the card.

```
scripts/phi-wait-vfio.sh [-c N] [SECONDS]
```

## Why it exists

With lingering on, a unit can start before `systemd-modules-load` has
inserted `vfio_pci`, before it has claimed the card from the `ids=`
option, and before udev has applied the group-phi ownership to
`/dev/vfio/<group>`. Those three happen asynchronously and in that order;
the script polls for the end state instead of trying to order the unit
against any of them.

It is a script and not an `ExecStartPre` one-liner because systemd
expands `$` in Exec lines itself, so shell with `$(...)` in it has to be
escaped into illegibility. And it exits non-zero on failure on purpose: a
unit whose `ConditionPathExists` fails is marked *skipped*, which
`Restart=on-failure` never acts on.

## Which card

The index is turned into an address by `phictl cards --plain` (through
`phi-env.sh`), so the unit and the daemon agree on which device card N
is. On a fresh clone where `phictl` is not built yet it falls back to the
Nth `8086:225d` device by address.

On failure it says which of the three is missing, because each has a
different fix: not bound (`sudo scripts/bind-vfio.sh`, then
`setup-arch.sh` so it sticks), no IOMMU group (firmware), or a node that
exists but is not writable (group membership is read at login).
