# Reproducing this project on Arch Linux

The walkthrough from a fresh clone to a booted card with SSH, in the order
the pieces depend on each other. Every command is one that exists in the
repository today (2026-09-19) with the flags shown; steps marked
**(recorded)** were run on this machine and their output is in
`docs/results/`, steps marked **(verified against the live card)** were
checked on 2026-09-17 against a card booted by `phi.service`, and steps
marked **(not re-run)** are transcribed from the scripts and their sibling
docs without a fresh run during this revision.

Target: a fresh Arch Linux (or derivative such as CachyOS) install with one
Xeon Phi 3120A in a CPU-direct PCIe slot, 16 threads and 64 GiB of RAM are
what the durations below were measured on, about 20 GB of disk under
`~/.cache` for the toolchain and kernel trees, plus the size of the card's
disk image.

## 0. Firmware settings

- **Above 4G Decoding: Enabled.** The card's BAR0 is 16 GiB and cannot be
  placed below 4 GiB (`docs/hardware.md`). Without it the host kernel logs
  `BAR 0: no space for` and the card is unusable.
- **IOMMU: Enabled** (AMD-Vi or VT-d). VFIO requires it. On Intel hosts add
  `intel_iommu=on` to the kernel command line if `/sys/kernel/iommu_groups`
  stays empty.
- Resizable BAR is irrelevant; the card is Gen2 and its BAR size is fixed by
  its flash (SSDG 328207-002, 2.1.12).

## 1. Clone

```sh
git clone https://github.com/Lasimeri/Intel-Phi-3120A.git "Intel Phi 3120A"
cd "Intel Phi 3120A"
```

The directory name contains spaces on purpose (it is the project's name);
every script quotes its paths and the toolchain scripts build under a
space-free alias in `~/.cache` (`toolchain/env.md`). A checkout at a path
without spaces works the same.

## 2. Host packages and system configuration (recorded)

```sh
sudo scripts/setup-arch.sh
```

Idempotent. It installs the packages listed in `scripts/setup-arch.md`,
creates the `phi` group and adds the invoking user to it, installs the udev
rule that gives `/dev/vfio/<group>` to that group, raises `memlock` for the
group (VFIO pins DMA memory), loads `vfio-pci` and `vfio_iommu_type1` at
boot (`/etc/modules-load.d/phi-vfio.conf`) and tells `vfio-pci` to claim
`8086:225d` at load time (`/etc/modprobe.d/phi-vfio.conf`), so a rebooted
host should need no binding step. **Log out and back in** so the group
applies (`id` must list `phi`).

The `modprobe.d` file was added on 2026-09-14. A host set up before that
has the `modules-load.d` file but not this one, so `vfio-pci` loads at
boot with no ids, claims nothing, and the card comes up with no driver;
re-run the script once to fix it. That happened on the development host on
2026-09-19 and `scripts/setup-arch.md` records the symptom, the three
checks and the repair. Note that the option leaves no sysfs file to
inspect: use `modprobe --showconfig`, not
`/sys/module/vfio_pci/parameters/ids`. Reboot survival with the file in
place is still unobserved.

`PHI_RUSTUP=1 sudo scripts/setup-arch.sh` installs `rustup` instead of the
distro `rust`; the project needs neither nightly nor rustup (ADR 0007), the
distro `rust` plus `rust-src` is the tested path.

## 3. Verify the card (recorded)

```sh
scripts/verify-card.sh
```

Prints the BDF, vendor/device/subsystem, link speed and width, BAR
assignments, AER counters, IOMMU group membership and reserved regions, and
whether a driver is bound. Non-zero exit if anything disqualifying is found
(no device, no BAR0, no BAR4, no IOMMU group, no `vfio-pci` module). The
output on this host is in `scripts/verify-card.md`.

## 4. Bind the card to vfio-pci (recorded)

With `/etc/modprobe.d/phi-vfio.conf` in place from step 2, a reboot should
leave the card already bound (`driver vfio-pci` in step 3). Check it rather
than assume it: `readlink /sys/bus/pci/devices/<BDF>/driver`. Before that
reboot, when the file is missing, or after `unbind`:

```sh
sudo scripts/bind-vfio.sh           # uses the detected BDF, or PHI_BDF, or an argument
sudo scripts/bind-vfio.sh unbind    # returns it to no driver
```

## 5. Host tools and first contact (recorded)

```sh
make build                          # cargo build of host/, debug profile, a few minutes the first time
host/target/debug/phictl info       # PCI identity, POST code, SPAD2, platform word, scratchpads
```

`phictl info` opens the VFIO group, maps the MMIO BAR and prints the POST
code and scratchpad registers. POST `"12"` means the bootstrap is waiting for
an OS image; opening the device resets the card, so the first read after an
open shows the bootstrap re-running GDDR training for about 9 s
(`docs/results/2026-09-13-reset-2.md`). VFIO admits one opener: while
`phictl boot` (or `phi.service`) holds the card, `phictl info` fails with a
busy device.

`make check` runs the documentation lint, `cargo fmt --check`, `clippy`,
the build, the tests and the C layout check (`Makefile`); none of it needs
the card.

## 6. Card toolchain (recorded, 2026-09-13)

Ordered in `toolchain/README.md`; every script sources `toolchain/env.sh`,
can be rerun and audits what it produces with `phi-isa-audit`. Build
products live under `~/.cache/intel-phi-3120a-build/` behind the symlinks
`toolchain/build`, `card/kernel/build`, `card/userland/build`,
`card/initramfs/build`.

| Step | Command | Duration on 16 threads | Result |
| --- | --- | --- | --- |
| 1 | `toolchain/llvm/build.sh all` | 55 min first time, 15 min clean rebuild (`docs/results/2026-09-13-p2-rust.md`) | patched clang and lld in `toolchain/build/llvm/` |
| 2 | `toolchain/musl/build.sh` | minutes | the sysroot at `toolchain/build/sysroot/` |
| 3 | `toolchain/compiler-rt/build.sh` | 2 min | builtins in clang's resource directory |
| 4 | `toolchain/libunwind/build.sh` | 1 min | `libunwind.a` in the sysroot |
| 5 | `toolchain/check/run.sh` | seconds | prints `phase P2 check: PASS` (C half) |
| 6 | `PHI_LLVM_VARIANT=dylib toolchain/llvm/build.sh all` | 12 min clean | `libLLVM.so.22.1` for rustc in `toolchain/build/llvm-dylib/` |
| 7 | `toolchain/rust/gen-target.sh` then `toolchain/rust/build-std.sh` | under a minute | `core`, `alloc`, `std` for the card; one `sse2` warning per crate is expected output |
| 8 | `toolchain/check/run.sh` | seconds | `rust_hypot=5` and a clean audit of the linked binary |
| 9 | `toolchain/libcxx/build.sh` | a few minutes | `libc++.a`, `libc++abi.a` in the sysroot |
| 10 | `PHI_LLVM_VARIANT=card toolchain/llvm/build.sh configure` then `build` | not recorded | clang, lld and the LLVM binutils built for the card (Canadian cross) |
| 11 | `card/userland/components/clang.sh` | not recorded | `card/userland/build/clang/phi-clang.tar.gz` (84 MB), the native toolchain package |

Step 6 needs the distro `rust` and `rust-src` packages (`setup-arch.sh`
installs them). Step 7 is rerun after every rustc upgrade. Steps 9 to 11
are only needed for compiling on the card (phase P7); everything up to
step 8 is needed for the kernel and the initramfs.

## 7. Card userland components (recorded for busybox, dropbear, agent)

```sh
card/userland/components/busybox.sh      # static busybox, audited, run on the host as a smoke test
ssh-keygen -t ed25519 -N '' -f ~/.ssh/phi_ed25519   # the card key; no passphrase, see docs/howto/secure-access.md
card/userland/components/dropbear.sh     # static dropbear, audited; generates the card's host keys once
card/agent/build.sh                      # phi-agent, Rust, needs steps 6 and 7 of the toolchain
```

Optional components, not part of the default image: `zlib.sh` and
`ncurses.sh` (into the sysroot), `cpython.sh` (a static CPython 3.14.7
package; the host needs `python` of the same version and `zstd`). gcc, tcc
and QuickJS have design notes only (`card/userland/components/*.md`).

## 8. Kernel (recorded, 2026-09-13)

```sh
card/kernel/build.sh all        # fetch v7.2.3 (shallow), apply the 28 patches, configure, build, audit
```

About 6 minutes clean. Output: `card/kernel/build/out/arch/x86/boot/bzImage`
and `vmlinux`. The audit must end with `0 illegal, 0 suspect` plus the
documented ignored sites (`card/kernel/build.md`).

## 9. Initramfs (recorded)

```sh
card/initramfs/build.sh
```

Seconds. Assembles busybox, `init`, dropbear with its host keys, root's
`authorized_keys` (from `~/.ssh/phi_ed25519.pub` and the usual `id_*.pub`,
or `PHI_SSH_PUBKEYS`), `phi-agent`, and anything under
`card/initramfs/extra/`, audits every ELF, packs
`card/initramfs/build/initramfs.cpio.gz` (mode 0600: it carries the host
keys). The initramfs and the host tools must be rebuilt together after a
change to `phi-rpc` (ADR 0009).

## 10. Disk image (recorded, 2026-09-16)

```sh
scripts/phi-disk.sh create /path/to/disk.img 256G    # sparse, ext4, mode 0600, seconds
```

The card mounts it on `/data` and binds `/opt/phi`, `/root` and `/home`
from it, so the native toolchain and everything built on the card survive
reboots. Set `PHI_DISK=/path/to/disk.img` in the environment or pass
`--disk` to `phi-up.sh`.

## 11. First boot, from an unprivileged shell (recorded, 2026-09-14 to 09-16)

```sh
scripts/phi-up.sh --ssh --disk /path/to/disk.img    # or PHI_DISK; --toolchain also loads clang
scripts/phi-run.sh nproc                            # 228
scripts/phi-run.sh sh -c 'cat /proc/mounts | grep phiblk0'
tail -f "$XDG_RUNTIME_DIR/phictl/console.log"       # the card's console
scripts/phi-down.sh                                 # poweroff (POST "KH"), then release
```

`phi-up.sh` starts `phictl boot` in the background with the kernel and
initramfs above, the control socket in `$XDG_RUNTIME_DIR/phictl/`, the SSH
forwarder on `127.0.0.1:2222` (`--ssh`) and the disk, and waits for the
agent (about 15 s: 9.5 s of GDDR training after the VFIO open, 4 s of
kernel, then init). `--toolchain` pushes `phi-clang.tar.gz` through the
socket (25 s); with a disk image this is needed once per image, not per
boot. Extra arguments go to `phictl boot` (`--host-mem 4G` gives the card
4 GiB of host RAM as swap, `--no-dma` serves the disk through the
aperture). The manual, foreground equivalent is in
`docs/howto/build-and-run.md`.

## 12. SSH (recorded, 2026-09-15)

```sh
ssh -p 2222 root@localhost 'uname -a; nproc'       # through the forwarder, no root anywhere
scp -P 2222 file root@localhost:/tmp/
```

For a stable name, pinned host key and the dedicated key only, the stanza
used on this machine (`docs/howto/secure-access.md`):

```
Host phi-fwd
    HostName 127.0.0.1
    Port 2222
    User root
    IdentityFile ~/.ssh/phi_ed25519
    IdentitiesOnly yes
    UserKnownHostsFile ~/.ssh/known_hosts_phi
    StrictHostKeyChecking yes
```

Pin the key once with `ssh-keyscan -p 2222 -t ed25519 127.0.0.1 >> ~/.ssh/known_hosts_phi`
while the card is up. A card that is a real interface on the host
(`10.9.0.2` on a TAP device) needs a root boot with `--net phi0`; the
forwarder is the default because it needs no root.

## 13. Autoboot at login (recorded, 2026-09-16; verified against the live card)

```sh
scripts/phi-autoboot.sh install /path/to/disk.img 4G   # writes and starts ~/.config/systemd/user/phi.service
systemctl --user status phi.service
journalctl --user -u phi.service -f                     # the console
scripts/phi-autoboot.sh remove
```

The unit runs the same `phictl boot` as `phi-up.sh` (socket, forwarder,
disk, 4 GiB of host memory), starts with the user's first session and stops
with the last one (a clean power-off through the socket, then the VFIO
release resets the card). `phi-up.sh` refuses to start while the service
holds the card; `systemctl --user stop phi.service` hands it back.

## 14. Monitoring (verified against the live card)

```sh
host/target/debug/phitop                 # full-screen viewer; --batch N for plain text
host/target/debug/phictl sensors         # from the SBOX, no card involvement
host/target/debug/phictl traffic         # PCIe bytes moved by the daemon
scripts/phi-run.sh cat /sys/class/hwmon/hwmon0/temp1_input   # on the card, millidegrees
```

Details and the numbers in `docs/howto/monitoring.md`.

## 15. Compile on the card (recorded, 2026-09-14)

With the toolchain on the disk (`--toolchain` once):

```sh
host/target/debug/phictl put prog.c /tmp/prog.c
host/target/debug/phictl exec -- sh -c 'cd /tmp && cc -O2 -o prog prog.c && ./prog'
```

`cc`, `c++`, `ld`, `ar`, `nm`, `objdump`, `strip` are in `/opt/phi/bin`,
first on the PATH of every `phictl exec`; the same works over SSH.

## Reference material (optional)

```sh
make vendor
```

Downloads MPSS 3.8.6 archives from archive.org, Intel's k1om kernel tree from
the solros repository, the v5.9 mainline mic driver, and the Intel PDFs into
`vendor/`, recording SHA-256 sums on first download and verifying them on
later runs (`scripts/fetch-vendor.md`). Nothing in the build depends on
`vendor/`.

## Recording results

Any result measured on hardware goes into `docs/results/<date>-<topic>.md`
with: date, host kernel version (`uname -r`), the card kernel and host
commit, and the exact command. `docs/README.md` lists them.
