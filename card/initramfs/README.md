# card/initramfs

The card's whole root filesystem is a gzip cpio archive built on the host
and placed at 128 MiB by `phictl boot --initrd`. There is no other root:
the disk image (`/dev/phiblk0`) is mounted on top of it at `/data`, and
bind mounts move `/etc`, `/var`, `/opt/phi`, `/root` and `/home` onto the
disk, so configuration, logs and the on-card toolchain persist.

| File | Role |
| --- | --- |
| `build.sh` | Assembles the tree and packs it. `build.md` is the detailed account. |
| `init` | PID 1 on the card. A busybox shell script, not a compiled program. |
| `extra/` | Anything dropped in here lands at the same path on the card (`extra/opt/hello` becomes `/opt/hello`). Git-ignored except for its README. |

## What is in the image

```
/init                 the shell script in this directory, mode 0755
/bin/busybox          the card busybox, plus 394 applet symlinks (one per applet)
/bin/phi-agent        card/agent/build.sh, the card end of phictl exec/put/get/status
/bin/dropbearmulti    plus links dropbear, dropbearkey, dbclient, scp, ssh
/lib/phi/etc-skel/    the card's /etc: passwd, group, shells, os-release,
                      hostname, hosts, profile, fstab, TZ, resolv.conf,
                      dropbear/ host keys, root-ssh/authorized_keys
/usr/{bin,sbin,lib}   empty; busybox installs its links in /bin only
/etc /var /run /opt /home /root /srv /mnt /media /proc /sys /dev /tmp
                      empty mount points and FHS directories
<anything under extra/>       copied in at the same path
```

The `/etc` skeleton is installed by `init`, not shipped at `/etc`, because
`/etc` is a bind mount from the persistent disk by the time it is needed.
`build.md` has the two categories: what is copied every boot (the files that
decide who may log in) and what is seeded once and then belongs to the card.

Built with `bsdtar --format newc --uid 0 --gid 0 | gzip -9`; the kernel
config enables `RD_GZIP` only. The archive is `chmod 600` because it
carries the card's SSH host private keys. Current size is about 1.4 MB.

Every ELF file that goes in, busybox and each file under `extra/`, is run
through `phi-isa-audit` first, so a binary built with the wrong flags
never reaches the card.

## What is not in the image

- No kernel modules. The project builds none: the tty, netdev, rpc and
  block devices are kernel patches 0016, 0021, 0022, 0025 and 0026
  (`card/drivers/phinet/README.md` explains the directory name).
- No compiler. The card's clang lives on the persistent disk under
  `/opt/phi`, pushed there once by
  `card/userland/components/clang-push.sh` (84 MB of tarball). Keeping it
  out is what holds the image near 1.4 MB. gcc, tcc and QuickJS are not
  built at all, and the static CPython is a component for a sibling
  project, not part of any image (`docs/plan.md`, P8 and P9).
- No `/etc/phi`. The ring base, the card address and the host address all
  arrive on the kernel command line or as environment overrides
  (`PHI_CARD_ADDR`).
- No `getty`, no `login`, no `crond`, no watchdog. Those exist on a standard
  install for an unattended multi-user machine. The card runs only while its
  owner is logged in to the host, and there are exactly two ways in: the
  control socket and the loopback SSH forward.

## What init does

In order: mount `proc`, `sysfs`, `devtmpfs`, `tmpfs`, `devpts`, `/run` and
`/dev/shm`; wait up to 5 s for `/dev/phiblk0` and mount it ext4 on `/data`,
then bind `/data/{etc,var,opt/phi,root,home}` over their mount points;
install `/etc` from the skeleton and set the hostname and `TZ` from it;
create the `/var` tree and link `/var/run` and `/var/lock` into `/run`;
start `syslogd` and `klogd` on `/var/log/messages`; wait up to 3 s for
`/dev/phiblk1` and `mkswap`/`swapon` it (host memory is volatile, so it is
reformatted every boot); bring up `lo` and `phi0`; start `dropbear -s` bound
to the card's own address (public keys only, root has no password) when the
image has it and `phi0` exists; start `phi-agent` on `/dev/phirpc`; print
the banner; then supervise an interactive login shell on the console through
`setsid` and `cttyhack`, so job control and Ctrl-C work over the ring and
`/etc/profile` is read.

PID 1 stays the script. It traps the signals busybox's `poweroff`, `halt`
and `reboot` send to PID 1 (USR2, USR1, TERM) and runs an orderly shutdown:
stop the services, `swapoff`, detach the bind mounts, remount `/data`
read-only and unmount it, stop the agent, and only then `poweroff -f`, which
the KNC platform layer answers with POST `KH` and a halt. A shell that exits
is respawned rather than taking init down. `build.md` explains why each step
is in that order; `docs/results/2026-09-19-card-os.md` has the three bugs
that stood between this and a filesystem that does not replay its journal on
every boot.

## Build and boot

```sh
card/userland/components/busybox.sh     # once
card/userland/components/dropbear.sh    # once, for SSH
card/agent/build.sh                     # once, for phictl exec
card/initramfs/build.sh
```

```sh
phictl boot --kernel card/kernel/build/out/arch/x86/boot/bzImage \
  --initrd card/initramfs/build/initramfs.cpio.gz \
  --cmdline "earlyprintk=phiring console=ttyPHI0"
```

`card/initramfs/build` is a symlink into `~/.cache/intel-phi-3120a-build`
(`toolchain/env.md` explains why the build trees are not in the source
tree).
