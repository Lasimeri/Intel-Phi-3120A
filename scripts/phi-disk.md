# phi-disk.sh

Creates and checks the card's persistent disk image. The image is a sparse
file (only written blocks take space) formatted ext4 on the host with
`mkfs.ext4 -F`, which needs no root and no loop device; the card mounts it
through `/dev/phiblk0` (kernel patch 0025) when `phictl boot --disk PATH`
serves it. On this machine the image is `/mnt/1TB-NVMe/phi/disk.img`, 256
GiB, on the WD Black SN850X mounted at `/mnt/1TB-NVMe` (xfs); `phi-up.sh`
picks it up from `PHI_DISK` or `--disk`.

- `create PATH SIZE`: refuses to overwrite; mode 0600, label `phidata`,
  lazy inode and journal initialisation so a 256 GiB image formats in
  seconds and uses a few MiB.
- `check PATH`: read-only `e2fsck -fn`, refused while a `phictl boot` runs
  (the card has it mounted; a check would see an inconsistent snapshot).
- `usage PATH`: apparent against allocated size.

One card at a time: the host tool opens the image read/write and the card
mounts it read/write; a second boot against the same image while another
is running would corrupt it, which `phi-up.sh` prevents by refusing to
start a second `phictl boot`.
