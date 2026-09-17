# card/initramfs/build.sh

Assembles `card/initramfs/build/initramfs.cpio.gz` from the card busybox
(`card/userland/components/busybox.sh`, audited again here) and `init`:

- `bin/busybox` plus one symlink per applet, laid out by the card binary
  itself (it runs on the host; same instruction subset).
- `init` mounts proc, sysfs, devtmpfs and tmpfs, prints the kernel, CPU
  count, model name and memory, and supervises a shell on the console (started through `setsid` and
  `cttyhack`, so job control and Ctrl-C work over the ring); PID 1 stays the
  script, traps busybox's poweroff, halt and reboot signals (USR2, USR1,
  TERM) into `poweroff -f`, which the kernel answers with POST `KH` and a
  halt, and respawns the shell when one exits.
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
- `etc/passwd` and `etc/group` for root only.
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
