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
  halt, and respawns the shell when one exits
  tty.
- `etc/passwd` and `etc/group` for root only.
- Packed with `bsdtar --format newc` (libarchive, part of Arch base) and
  gzip; no Python anywhere.

Boot: `phictl boot --initrd card/initramfs/build/initramfs.cpio.gz --cmdline
"earlyprintk=phiring console=ttyPHI0 nosmp"`. The loader places the
archive at 128 MiB (Intel's slot) and patches the bzImage header. With
`console=ttyPHI0` the kernel console moves from the boot console to the
tty driver (patch 0016), which is what gives init a `/dev/console`; what
you type into `phictl console` arrives on it.
