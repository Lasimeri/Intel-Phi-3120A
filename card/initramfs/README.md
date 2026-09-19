# card/initramfs

The card's whole root filesystem is a gzip cpio archive built on the host
and placed at 128 MiB by `phictl boot --initrd`. There is no other root:
the disk image (`/dev/phiblk0`) is mounted on top of it at `/data`, and
bind mounts move `/opt/phi`, `/root` and `/home` onto the disk.

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
/etc/passwd, group    root only, shell /bin/sh
/etc/dropbear/        ed25519 and RSA host keys, generated once on the host
/root/.ssh/authorized_keys   the user's public keys (PHI_SSH_PUBKEYS)
/proc /sys /dev /tmp /sbin    empty; init mounts the first four, /sbin is
                      created but busybox installs its links in /bin only
<anything under extra/>       copied in at the same path
```

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

## What init does

In order: mount `proc`, `sysfs`, `devtmpfs`, `tmpfs` and `devpts`; set
the hostname; wait up to 5 s for `/dev/phiblk0` and mount it ext4 on
`/data` with the three bind mounts; wait up to 3 s for `/dev/phiblk1` and
`mkswap`/`swapon` it (host memory is volatile, so it is reformatted every
boot); bring up `lo` and `phi0`; start `dropbear -s -p 22` (public keys
only, root has no password) when the image has it; start `phi-agent` on
`/dev/phirpc`; print the kernel version, CPU count, model and memory;
then supervise an interactive shell on the console through `setsid` and
`cttyhack`, so job control and Ctrl-C work over the ring.

PID 1 stays the script. It traps the signals busybox's `poweroff`,
`halt` and `reboot` send to PID 1 (USR2, USR1, TERM) and turns them into
`poweroff -f`, which the KNC platform layer answers with POST `KH` and a
halt; a shell that exits is respawned rather than taking init down.

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
