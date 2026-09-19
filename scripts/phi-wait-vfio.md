# phi-wait-vfio.sh

Blocks until the card is usable through VFIO, or fails with a message that
names which of the three preconditions is missing.

```sh
scripts/phi-wait-vfio.sh [SECONDS]      # default 60; PHI_BDF selects the card
```

Success means all three of:

1. the card is bound to `vfio-pci` (`/sys/bus/pci/devices/<BDF>/driver`),
2. it has an IOMMU group,
3. `/dev/vfio/<group>` exists and is writable by this process.

## Why it exists

`phi.service` used to carry `ConditionPathExists=/dev/vfio/vfio`. That is
wrong twice over once the unit starts at host boot rather than at login:

- **A failed condition marks a unit skipped, not failed.** `Restart=` never
  acts on a skip, so the unit would sit in a state that reads like success
  and do nothing. That is a quieter version of the `activating
  (auto-restart)` trap documented in `phi-autoboot.md`.
- **It tested the wrong thing.** `/dev/vfio/vfio` is the container node; it
  appears as soon as the `vfio` module loads and says nothing about this
  card being bound or the group node being ours.

At host boot three things happen asynchronously and in no guaranteed order
relative to the user manager: `systemd-modules-load` inserts `vfio_pci`, the
`ids=8086:225d` option makes it claim the card, and udev applies the
group-`phi` ownership from the rule `setup-arch.sh` installs. Rather than
trying to order a user unit against system units, which systemd does not
offer, this polls for the end state once a second and exits non-zero on
timeout, which `Restart=on-failure` does act on.

## Why a script and not an `ExecStartPre` one-liner

systemd expands `$` in `Exec` lines itself, so a shell snippet using
`$(...)` and `$var` has to be escaped past legibility, and a mistake there
fails at boot rather than at edit time. A file also gets tested on its own:

```sh
scripts/phi-wait-vfio.sh 5                     # 0 immediately when the card is ready
PHI_BDF=0000:00:03.1 scripts/phi-wait-vfio.sh 3   # 1, "bound to 'pcieport', not vfio-pci"
PHI_BDF=0000:ff:ff.0 scripts/phi-wait-vfio.sh 2   # 1, "not a PCI device on this host"
```

## The messages

Each failure names a different fix, because each has one:

| Message | Fix |
| --- | --- |
| bound to `<something>`, not vfio-pci | `sudo scripts/bind-vfio.sh` now, `sudo scripts/setup-arch.sh` to make it stick |
| no IOMMU group | the IOMMU is off in firmware or on the kernel command line |
| `/dev/vfio/<group>` never appeared | the group node is not being created; check the udev rule |
| exists but is not writable | group `phi` membership is read at process start, so log out and back in after `setup-arch.sh` |

Note the last one carefully: it cannot be fixed by waiting longer, which is
why it is reported separately from the timeout rather than folded into it.
