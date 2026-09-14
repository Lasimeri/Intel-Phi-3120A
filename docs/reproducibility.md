# Reproducing this project on Arch Linux

Target: a fresh Arch Linux (or derivative such as CachyOS) install with one
Xeon Phi 3120A in a CPU-direct PCIe slot, Above-4G decoding enabled in
firmware, and an IOMMU (AMD-Vi or VT-d) enabled in firmware.

## 0. Firmware settings

- **Above 4G Decoding: Enabled.** The card's BAR0 is 16 GiB and cannot be
  placed below 4 GiB. Without this the kernel logs `BAR 0: no space for` and
  the card is unusable.
- **IOMMU: Enabled** (AMD-Vi / VT-d). VFIO requires it.
- Resizable BAR is irrelevant; the card is Gen2 and its BAR size is fixed by
  its flash (SSDG 2.1.12).

## 1. Packages and system configuration

```sh
sudo scripts/setup-arch.sh
```

The script is idempotent and does, in order:

1. `pacman -S --needed` for: `base-devel`, `rust` (or `rustup` if you opt in),
   `clang`, `lld`, `llvm`, `tcc`, `pciutils`, `poppler` (for `pdftotext`),
   `git`, `curl`, `cpio`, `bc`, `flex`, `bison`, `libelf`, `openssl`, `perl`.
2. Installs `/etc/udev/rules.d/80-phi-vfio.rules` so `/dev/vfio/<group>` is
   owned by the `phi` group, and creates that group.
3. Installs `/etc/security/limits.d/80-phi.conf` raising `memlock` for the
   `phi` group (VFIO pins DMA memory).
4. Installs `/etc/modules-load.d/phi-vfio.conf` so `vfio-pci` loads at boot.

Add your user to the `phi` group and log in again.

## 2. Verify the card

```sh
scripts/verify-card.sh
```

Prints the BDF, vendor/device/subsystem, link speed and width, BAR
assignments, AER counters, IOMMU group membership and reserved regions, and
whether a driver is bound. Exit code is non-zero if anything disqualifying is
found (no BAR0, link down, no IOMMU group).

## 3. Bind the card to vfio-pci

```sh
sudo scripts/bind-vfio.sh          # uses the detected BDF
sudo scripts/bind-vfio.sh unbind   # returns it to no driver
```

Uses `driver_override` and `drivers_probe`, the standard sysfs mechanism.

## 4. Build and first contact

```sh
make build
PHI_BDF=0000:2e:00.0 host/target/debug/phictl info
```

`phictl info` opens the VFIO group, maps the MMIO BAR, and prints the POST
code and scratchpad registers. POST code `0x12` means the bootstrap is waiting
for an OS image (Intel readme for MPSS 2.1, POST code table).

## 5. Reference material (optional)

```sh
make vendor
```

Downloads MPSS 3.8.6 archives from archive.org, Intel's k1om kernel tree from
the solros repository, the v5.9 mainline mic driver, and the Intel PDFs into
`vendor/`, verifying SHA-256 where a known-good hash exists. Nothing in the
build depends on `vendor/`.

## 6. Card-side toolchain and kernel

The card toolchain is scripted and ordered in `toolchain/README.md` (eight
steps, about two hours of machine time, everything under
`~/.cache/intel-phi-3120a-build/`); `toolchain/check/run.sh` is its exit
test and prints `phase P2 check: PASS`. The kernel steps live under `card/`
and are marked in `docs/plan.md` as they are implemented.

## Recording results

Any result measured on hardware goes into `docs/` with: date, host kernel
version (`uname -r`), card POST code before and after, and the exact command.
