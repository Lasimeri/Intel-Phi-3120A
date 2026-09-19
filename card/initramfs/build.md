# card/initramfs/build.sh

Assembles `card/initramfs/build/initramfs.cpio.gz` from the card busybox
(`card/userland/components/busybox.sh`, audited again here) and `init`:

- `bin/busybox` plus one symlink per applet, laid out by the card binary
  itself (it runs on the host; same instruction subset).
- `init` mounts the pseudo filesystems, installs `/etc` from the skeleton,
  starts the system log, prints the kernel, CPU count, model name and
  memory, and supervises a login shell on the console (started through
  `setsid` and `cttyhack`, so job control and Ctrl-C work over the ring, and
  with `-l` so it reads `/etc/profile`); PID 1 stays the script, traps
  busybox's poweroff, halt and reboot signals (USR2, USR1, TERM) into the
  orderly shutdown described below, and respawns the shell when one exits.
- `init` also brings up `lo` and `phi0` (`10.9.0.2/24`, or `PHI_CARD_ADDR`),
  mounts devpts for SSH sessions, and starts dropbear when the image has it
  (`dropbear -s -p 22`: public-key logins only; root has no password).
- dropbear, when `card/userland/components/dropbear.sh` has built it: the
  static multi-call binary and its links (`dropbear`, `dropbearkey`,
  `dbclient`, `scp`, `ssh`), host keys under `/etc/dropbear`, and the
  user's public keys as `/root/.ssh/authorized_keys` (`PHI_SSH_PUBKEYS`
  overrides the default `~/.ssh/id_*.pub` list).
- `phi-agent` (`card/agent/build.sh`), started by `init` on `/dev/phirpc`:
  the card end of `phictl exec`, `put`, `get` and `status`
  (`docs/decisions/0008-direct-access-tool.md`).
- Files under `card/initramfs/extra/` are copied in at the same paths
  (`extra/opt/hello` becomes `/opt/hello`), each ELF audited first; the
  directory is git-ignored except for its README. This is how programs
  built with the card toolchain reach the card until phase P5 brings a
  network (`docs/howto/build-and-run.md`).
- Packed with `bsdtar --format newc` (libarchive, part of Arch base) and
  gzip; no Python anywhere.

Boot: `phictl boot --initrd card/initramfs/build/initramfs.cpio.gz --cmdline
"earlyprintk=phiring console=ttyPHI0 nosmp"`. The loader places the
archive at 128 MiB (Intel's slot) and patches the bzImage header. With
`console=ttyPHI0` the kernel console moves from the boot console to the
tty driver (patch 0016), which is what gives init a `/dev/console`; what
you type into `phictl console` arrives on it.

## Persistent storage (2026-09-16)

`init` waits up to 5 s for `/dev/phiblk0`, the block device the host
serves from a disk image (`phictl boot --disk`, kernel patch 0025), mounts
it as ext4 on `/data` and bind-mounts `/data/opt/phi`, `/data/root` and
`/data/home` over the image's `/opt/phi`, `/root` and `/home`. Root's
`.ssh` directory is copied from the image into `/data/root` first on every
boot, so the authorized keys baked into the initramfs stay authoritative.
The toolchain pushed once with `clang-push.sh` then survives reboots.
Without a disk the boot continues after the wait with everything in RAM as
before.

## Host memory (2026-09-16)

When the host serves memory (`phictl boot --host-mem SIZE`, kernel patch
0026), `/dev/phiblk1` appears within a few seconds of init; `init` runs
`mkswap` and `swapon` on it every boot (host RAM is volatile), which
extends the card's 6 GB by that size at DMA speed. `/dev/phihost` maps the
same memory for direct access.

## The /etc skeleton (2026-09-19)

`build.sh` puts the card's `/etc` in the image at `/lib/phi/etc-skel`, not
at `/etc`. `init` installs it after it has bound `/data/etc` over `/etc`,
which is what lets the two categories behave differently:

| Copied every boot | Seeded only when absent |
| --- | --- |
| `passwd`, `group`, `shells`, `os-release`, `dropbear/` (the host keys), and root's `authorized_keys` | `hostname`, `hosts`, `profile`, `fstab`, `TZ`, `resolv.conf` |

The left column decides who may log in, so a rebuilt image has to win over
whatever is on the disk. The right column is configuration a person edits on
the card, so it survives. Both live on the persistent disk between boots.

`profile` sets `PATH` to `/opt/phi/bin:/bin:/sbin:/usr/bin:/usr/sbin`, the
prompt, `PAGER`, `EDITOR`, and `CC`/`CXX` when the card's clang is present.
`TZ` holds a POSIX timezone string (`UTC0` by default): there is no zoneinfo
database on the card, so `CST6CDT,M3.2.0,M11.1.0` is the form to write for
US Central.

## Filesystem layout (2026-09-19)

The image carries `/usr/{bin,sbin,lib}`, `/var`, `/run`, `/srv`, `/mnt`,
`/media`, `/opt` and `/home` as well as `/bin`, `/sbin` and `/etc`, so a
program that hard-codes an FHS path finds it. busybox installs its applet
links in `/bin` only; `/sbin` and `/usr` stay empty and exist for anything
that looks there.

`init` mounts `/run` and `/dev/shm` as tmpfs, links `/var/run` to `/run` and
`/var/lock` to `/run/lock`, and creates `/var/{log,lib,spool,cache,tmp}`.
With a disk, `/etc`, `/var`, `/opt/phi`, `/root` and `/home` are bind mounts
from `/data`, so logs and configuration persist; without one they are
directories in the initramfs and everything is as volatile as before.

## System log (2026-09-19)

`init` starts `/bin/syslogd -O /var/log/messages -s 2048 -b 4` and
`/bin/klogd`, so kernel messages that scrolled past the console can be read
afterwards, and the log persists on the disk. They are started through the
applet symlinks and not as `busybox syslogd`: a process started the second
way has `comm` `busybox`, and `killall syslogd` at shutdown then matches
nothing, which left `/var/log/messages` open and the filesystem dirty
(`docs/results/2026-09-19-card-os.md`).

## Shutdown (2026-09-19)

`poweroff` (without `-f`) on the card, or `phi down` on the host, signals
PID 1, and `init` runs an ordinary shutdown: stop dropbear, klogd and
syslogd; `swapoff -a`; `sync`; detach the five bind mounts with `umount -l`
(the console shell's working directory is normally `/root`, a bind of the
same filesystem, so a plain `umount` of it fails); `mount -o remount,ro
/data`, which checkpoints the journal and clears `needs_recovery`
synchronously; `umount /data`; then stop `phi-agent` and call `poweroff -f`.

The agent is stopped last on purpose. The host's stop sequence waits for the
agent to stop answering before it ends `phictl`, and `phictl` is what serves
the block channel; killing the agent first ended that wait while the
superblock write was still in flight.

`poweroff -f` anywhere earlier in the chain defeats all of this: it calls
`reboot(2)` from the calling process and never signals init.

## Programs on the persistent disk, and the default PATH (2026-09-19)

`/opt/phi/bin` holds everything installed on the card that is not busybox:
the native clang (`card/userland/components/clang-push.sh`) and whatever the
sibling repositories push there (htop, fastfetch, glances and its CPython).
`/etc/profile` puts it first on `PATH`, which is enough for an interactive
login shell and for `phictl exec`.

It is not enough for everything, and the gap is the kind a user hits
immediately:

```
$ ssh phi              # interactive: a login shell, reads /etc/profile
phi:~$ fastfetch       # works

$ ssh phi fastfetch    # one command: a non-login shell, never reads it
sh: fastfetch: not found
```

`ssh host command` runs a non-login shell, and dropbear hands it
`/usr/sbin:/usr/bin:/sbin:/bin`. So `init` symlinks every executable in
`/opt/phi/bin` into `/usr/bin`, which is on that default. A program on the
card then behaves like a program on any other Linux: the shell finds it
whatever kind of shell it is. `/usr/bin` is in the initramfs, so the links
are rebuilt on every boot and always match what is actually installed; the
boot banner reports how many there were.

The links shadow busybox applets of the same name (`ar`, `nm`, `strip` and
so on), which is what a real toolchain installation does too, and matches
the order `/etc/profile` already set for login shells.

## The fastfetch configuration

`card/initramfs/etc-skel/fastfetch.jsonc` is installed as
`/etc/fastfetch/config.jsonc`, seeded once like the other editable files, so
a change made on the card survives. It is versioned here rather than in the
`fastfetch-phi` repository because it describes the card, not the program.

It is plain ASCII: the card's locale is `C` and its console is a tty over
the ring, so box-drawing characters would be noise there even though they
render over SSH. Beyond the usual modules it adds `command` modules that
read the card's hwmon device (kernel patch 0027) for the die temperature,
its recorded peak, the core clock and the core voltage, and it labels swap
as what it is: host RAM over PCIe, not a disk.

Note for anyone editing the build script: `chmod 644 "$skel"/*` must run
before the `fastfetch/` directory is created. `chmod 644` on a directory
strips its traverse bit and everything under it becomes unreachable, which
is what happened the first time.
