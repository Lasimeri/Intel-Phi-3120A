# phi-install.sh

One command from a fresh Arch Linux install to a running card.

```sh
git clone https://github.com/Lasimeri/Intel-Phi-3120A.git "Intel Phi 3120A"
cd "Intel Phi 3120A"
export PHI_DISK=/var/lib/phi/disk.img     # where the card's persistent disk lives
scripts/phi-install.sh
```

It does what `docs/reproducibility.md` describes by hand, in the same
order, calling the same scripts. It does not reimplement any build: every
stage runs the script a person would have run.

## The three things it is built around

**It checks before it does.** Every stage knows how to tell whether it is
already finished, by looking for the file it produces. So the command is
safe to run at any time: on a machine with nothing installed it does
everything, on a machine with everything installed it does nothing and says
so.

**It resumes.** A stage that fails stops the run and says which one broke.
Fix the cause, run the same command again, and the finished stages do not
repeat. This matters because the toolchain stages are 15 to 55 minutes and
nobody should pay that twice.

**It verifies what it claims.** After a stage runs, the check runs again. A
stage that exits 0 without producing its artifact is reported as a failure
rather than passed over, which is the failure mode that otherwise shows up
three stages later as something confusing.

## Commands

| | |
| --- | --- |
| `scripts/phi-install.sh` | run every pending stage in order |
| `scripts/phi-install.sh status` | a table of every stage, its state, and what it produces |
| `scripts/phi-install.sh run STAGE` | force one stage, finished or not |
| `scripts/phi-install.sh list` | the stage ids |

Options: `--full` also builds the card-native toolchain (phase P7, only
needed to compile *on* the card), `--dry-run` prints what would happen,
`--yes` does not pause before the long builds.

## The stages

In dependency order, which is also the order they run.

| stage | what it is |
| --- | --- |
| `packages` | host packages, the `phi` group, the udev rule, the memlock limit |
| `card` | the card is enumerated and its link is healthy |
| `vfio` | `vfio-pci` owns the card |
| `host-tools` | `phictl`, `phitop`, `phi-isa-audit`, the MVEX encoder |
| `llvm` | the patched clang and lld that can target this card |
| `sysroot` | musl, compiler-rt and libunwind for the card |
| `llvm-dylib` | `libLLVM.so` for rustc, needed to build card Rust |
| `rust-std` | `core`, `alloc` and `std` cross-built for the card |
| `userland` | static busybox and dropbear |
| `sshkey` | the ssh key the card will trust |
| `agent` | `phi-agent`, the card end of the control channel |
| `kernel` | the patched Linux kernel for the card |
| `initramfs` | the boot image |
| `disk` | a persistent disk image the card mounts on `/data` |
| `cli` | `phi`, `phictl` and `phitop` on `PATH` |
| `autoboot` | a user service that runs the card at host boot |

With `--full`, three more: `libcxx`, `llvm-card` and `clang-pkg`, which
together give the card its own clang.

## Root, and how little of it is needed

Only `packages` and `vfio` need root, and the script asks for `sudo`
**once, before the long builds start**, rather than stopping for a password
forty minutes in. Everything after that runs as the user: the `phi` group
grants the VFIO device and the control socket lives in the user's runtime
directory, so booting and using the card never needs `sudo` at all.

## Environment

| variable | meaning |
| --- | --- |
| `PHI_DISK` | where the card's persistent disk image lives. The `disk` stage refuses to guess. |
| `PHI_DISK_SIZE` | size of that image, default `64G`. It is sparse, so this is a ceiling and not an allocation. |
| `PHI_HOST_MEM` | host RAM given to the card as swap, default `6G` |

## After it finishes

```sh
phi up            # boot the card
phi run nproc     # 228
phi sh            # a shell on the card
phi top           # the live viewer
```

`scripts/phi.md` documents the rest.
