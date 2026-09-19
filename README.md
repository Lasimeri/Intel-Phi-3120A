# Intel Phi 3120A

A from-scratch software stack for one Intel Xeon Phi 3120A coprocessor
(Knights Corner, PCI `8086:225d`, subsystem `8086:3c98`), written in Rust
wherever Rust can run, with C only where the hardware or an upstream project
leaves no choice.

What it does today (2026-09-17, every item measured and recorded in
`docs/results/`):

1. The card boots a **mainline Linux 7.2.3** kernel with a 28-patch series
   (not Intel's 2.6.38 uOS) on all 57 cores x 4 threads = 228 hardware threads.
2. The host drives it from an unprivileged shell through a **Rust userspace
   daemon on VFIO**: no host kernel module, no MPSS.
3. You `ssh` into it (dropbear on the card, a userspace TCP forwarder in the
   daemon, no root needed), or run commands through a control socket
   (`phictl exec`).
4. You compile C and C++ **on the card** with a native clang 22, on a
   persistent disk served from a host image (846 MB/s reads through the
   card's DMA engine), with 6 GiB of host RAM as swap on top of the card's
   6 GB of GDDR5 (8 GiB allocated and verified byte for byte, 2026-09-19).
5. The card runs an ordinary Linux userland: FHS layout, a persistent `/etc`
   and `/var/log`, `syslogd` and `klogd`, a login shell that reads
   `/etc/profile`, and a shutdown that unmounts its disk cleanly. It comes
   up during host boot with nobody logged in, and is reachable only from
   this machine either way.
6. Sensors (die temperatures, core voltage and clock) read from the host and
   from the card (hwmon), hardware performance counters (`phiperf`), and a
   live resource viewer on the host (`phitop`).
7. The 512-bit vector unit is reachable through a project encoder
   (`knc-mvex`); the Mandelbrot iteration on the VPU runs at 1.47x the host's
   16 AVX2 threads.

Not done: gcc, tcc and QuickJS on the card (phases P8 and P9 are design notes
under `card/userland/components/`), `make` on the card, interrupts (every
transport polls), power readout (the SMC is not driven). CPython 3.14.7 is
cross-built as a static component for a sibling project (glances on the
card) and is not part of the default image.

Intel's MPSS (Manycore Platform Software Stack) is **not** ported. It is used
as a reference for the hardware/software contract only. Everything the host
runs is new code in this repository.

## Why this is hard, in one table

| Constraint | Consequence |
| --- | --- |
| KNC deletes CMOV, PAUSE, MONITOR/MWAIT, CMPXCHG16B, FCMOV/FCOMI, SYSENTER, IN/OUT, PREFETCH, CLFLUSH, fences, and every MMX/SSE/AVX instruction (ISA reference 327364-001, appendix B) | No stock compiler output runs. Every binary is audited (`phi-isa-audit`). The kernel and userland are built with a patched LLVM. |
| KNC has no 32-bit compatibility submode inside long mode (SSDG 328207-002, 4.2.3) | Userland must be 64-bit with a float ABI that does not use XMM registers: arguments on the stack, returns in x87 `ST0` (ADR 0002). |
| No PIT, RTC, HPET, ACPI, legacy PIC; I/O APIC at a 64-bit address; CR4.PGE faults (SSDG 4.2) | The kernel gets a small KNC platform layer modeled on mainline's jailhouse guest support. |
| The on-card bootstrap enters the kernel's 32-bit entry with SFI tables (SSDG 2.2.4) | A minimal SFI parser returns to the kernel (mainline removed SFI in 5.12). |
| The host kernel (7.x) has no Xeon Phi driver (mainline removed `drivers/misc/mic` in 5.10) | The host side is a Rust userspace daemon on VFIO. No kernel module on the host. |
| FXSAVE does not cover the vector registers (ISA reference B.4, B.5) | Kernel patch 0024 saves and restores the 2112 bytes of VPU state per task. |
| Bun requires SSE4.2 even in baseline builds | QuickJS is the JavaScript runtime (ADR 0004; not built yet). |

Full research record: [`docs/research/`](docs/research/README.md).
Decisions and their rationale: [`docs/decisions/`](docs/decisions/README.md).
The phased plan with exit criteria: [`docs/plan.md`](docs/plan.md).
Dated measurements: [`docs/README.md`](docs/README.md#results) lists them.

## Repository map

| Path | What lives there |
| --- | --- |
| `host/` | Rust workspace. Binaries: `phictl` (boot, console, control socket daemon, disk and host-memory service, SSH forwarder, sensors), `phitop` (live viewer), `phi-isa-audit` (flags KNC-illegal instructions in any x86-64 ELF), `knc-mvex-gen` (emits the vector-code files). Libraries: `phi-vfio` (VFIO device access, DMA mapping, PCIe byte counters), `phi-regs` (SBOX/DBOX register map, POST codes, bzImage header offsets), `phi-hw` (reset, boot, image loading, DMA engine), `phi-ring` (host side of the ring transport), `phi-rpc` (frames of the control channel, shared with the card agent), `knc-mvex` (MVEX encoder). |
| `card/` | Everything that runs on the card: `kernel/` (28-patch series against v7.2.3, config fragment, build script), `agent/` (`phi-agent`, Rust, the card end of the control socket), `initramfs/` (`init` and the assembly script), `userland/components/` (busybox, dropbear, zlib, ncurses, CPython build scripts; clang packaging; gcc, tcc, QuickJS notes), `examples/` (benchmarks and probes compiled on the card), `drivers/phinet/include/phi_ring.h` (the C mirror of the ring layout). |
| `toolchain/` | How code for the card is compiled: the LLVM patch series and build script (three variants), `knc-cc`/`knc-c++` wrappers, musl, compiler-rt, libunwind, libc++, the Rust target JSON and `build-std`, the phase P2 exit check. |
| `scripts/` | `phi.sh`, the one command a person uses (`phi up/run/sh/top/status/down`), plus what it drives: host setup (`setup-arch.sh`), card verification and VFIO binding, the card's daily drivers (`phi-up.sh`, `phi-run.sh`, `phi-down.sh`, `phi-disk.sh`, `phi-autoboot.sh`), reference fetching, documentation lint. Each script has a sibling `.md`. |
| `docs/` | `reproducibility.md` (the fresh-clone walkthrough), `hardware.md`, `plan.md`, `howto/` (build and run, direct access, monitoring, secure access), `spec/` (ring protocol, SBOX registers), `decisions/` (ADRs), `research/`, `results/` (dated measurements). |
| `tools/` | Two C helpers compiled with `tcc`: the ring layout cross-check and a boot-path bisection stub. |
| `vendor/` | Git-ignored. Reference material fetched by `scripts/fetch-vendor.sh` (MPSS 3.8.6 archives, Intel's k1om kernel tree, PDFs). Never committed, never linked into builds. |

## Where to start

1. [`docs/hardware.md`](docs/hardware.md): what the card is and how this host sees it.
2. [`docs/reproducibility.md`](docs/reproducibility.md): the walkthrough from a fresh Arch Linux install to a booted card with SSH, in order, with durations.
3. [`docs/howto/direct-access.md`](docs/howto/direct-access.md) and [`docs/howto/build-and-run.md`](docs/howto/build-and-run.md): daily use.
4. [`docs/spec/ring-protocol.md`](docs/spec/ring-protocol.md): the one transport everything rides on.
5. [`docs/decisions/`](docs/decisions/README.md) and [`docs/research/`](docs/research/README.md): why it is built this way.

## Quick start on Arch Linux

```sh
git clone https://github.com/Lasimeri/Intel-Phi-3120A.git "Intel Phi 3120A"
cd "Intel Phi 3120A"
sudo scripts/setup-arch.sh          # packages, phi group, udev rule, memlock, vfio-pci claims the card at boot
scripts/verify-card.sh              # the card, its link, BARs, IOMMU group
sudo scripts/bind-vfio.sh           # bind now; after a reboot vfio-pci should claim it itself (setup-arch.md)
make build                          # cargo build of the host workspace
host/target/debug/phictl info       # first contact: POST code and scratchpads (needs the card unheld)
```

Then the card side, in the order of `docs/reproducibility.md`: the toolchain
(`toolchain/README.md`, about two hours of machine time), the kernel
(`card/kernel/build.sh all`), the userland components, the initramfs
(`card/initramfs/build.sh`), a disk image (`scripts/phi-disk.sh create`),
and:

```sh
scripts/phi-autoboot.sh install /path/to/disk.img 6G   # run the card from a user service
scripts/phi-autoboot.sh at-boot                        # ...starting at host boot, not at login
scripts/phi.sh install-cli                             # ~/.local/bin/phi and fish completions
```

After that one command drives everything, from any shell, without `sudo`:

```sh
phi status            # unit, card, memory, disk, and who can reach it
phi run nproc         # 228; stdin, stdout, stderr and the exit status relayed
phi sh                # an interactive login shell on the card
phi top               # the live viewer (or just: phitop)
phi up / phi down     # start and stop; down unmounts the card's disk first
```

`scripts/phi.md` lists the rest. The scripts underneath (`phi-up.sh`, `phi-run.sh`,
`phi-down.sh`) still work on their own for a card booted by hand.

## Conventions

Every source file has a sibling Markdown file with the same stem that explains
the context the code cannot: why the file exists, which document or hardware
observation it depends on, and how to test it. `scripts/check-docs.sh`
enforces this, the no-dash rule and the relative links, along with the other
rules in [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

Project code is MIT (see `LICENSE.md`). Material under `card/kernel/` derives
from Linux and is GPL-2.0-only. Intel documents and MPSS archives referenced by
this project are not redistributed here.
