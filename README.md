# Intel Phi 3120A

A from-scratch software stack for Intel Xeon Phi 3120-series coprocessors
(Knights Corner, PCI `8086:225d`; up to 16 cards in one host, each
addressed by an index), written in Rust
wherever Rust can run, with C only where the hardware or an upstream project
leaves no choice.

What it does today (2026-09-22, every item measured and recorded in
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
8. **The card executes AVX-512 for a host that has none**: a program's
   AVX-512 machine code is rewritten into the card's instruction set, a
   resident worker runs it across the 57 vector units, and every result
   lane is bit-identical to AVX-512 hardware (316 GFLOP/s on card 0, 374 on
   card 1, 2026-09-22). A host-side software fallback runs unmodified
   AVX-512 binaries when the card is not worth the trip.
9. **Up to 16 cards in one host**, each addressed by an index that fixes
   its socket, port, subnet, hostname, disk and systemd instance; two are
   running today (`docs/howto/multiple-cards.md`).

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
| `host/` | Rust workspace. Binaries: `phictl` (boot, console, control socket daemon, disk and host-memory service, SSH forwarder, sensors), `phitop` (live viewer), `phi-isa-audit` (flags KNC-illegal instructions in any x86-64 ELF), `knc-mvex-gen` (emits the vector-code files). Libraries: `phi-vfio` (VFIO device access, DMA mapping, PCIe byte counters, and `cards`: the index-to-card mapping every tool shares), `phi-regs` (SBOX/DBOX register map, POST codes, bzImage header offsets), `phi-hw` (reset, boot, image loading, DMA engine), `phi-ring` (host side of the ring transport), `phi-rpc` (frames of the control channel, shared with the card agent), `knc-mvex` (MVEX encoder). AVX-512: `avx512-xlate` (EVEX to MVEX translator), `phi512` (the `LD_PRELOAD` library that performs AVX-512 in software on a host without it), `phi-vpu` (hands AVX-512 work to the card's vector units). |
| `card/` | Everything that runs on the card: `kernel/` (28-patch series against v7.2.3, config fragment, build script), `agent/` (`phi-agent`, Rust, the card end of the control socket), `initramfs/` (`init` and the assembly script), `userland/components/` (busybox, dropbear, zlib, ncurses, CPython build scripts; clang packaging; gcc, tcc, QuickJS notes), `examples/` (benchmarks and probes compiled on the card), `vpu/` (the resident AVX-512 co-processor worker and its host/card protocol), `drivers/phinet/include/phi_ring.h` (the C mirror of the ring layout). |
| `toolchain/` | How code for the card is compiled: the LLVM patch series and build script (three variants), `knc-cc`/`knc-c++` wrappers, musl, compiler-rt, libunwind, libc++, the Rust target JSON and `build-std`, the phase P2 exit check. |
| `scripts/` | `phi.sh`, the one command a person uses (`phi [-c N] cards/up/run/sh/top/status/vpu/down`), plus what it drives: host setup (`setup-arch.sh`), the one-command installer (`phi-install.sh`), card verification and VFIO binding for every card, the cards' daily drivers (`phi-env.sh` names what card N owns, `phi-boot.sh` assembles its boot line, `phi-up.sh`, `phi-run.sh`, `phi-down.sh`, `phi-disk.sh`, `phi-autoboot.sh` with the `phi@N` template), reference fetching, documentation lint, and the AVX-512 pieces (`phi-vpu.sh` deploys and drives the card worker; `phi512.sh`, `phi512-check.sh`, `phi512-install.sh`, `phi512-ground.sh` run, verify, install and ground the software path). Each script has a sibling `.md`. |
| `docs/` | `reproducibility.md` (the fresh-clone walkthrough), `hardware.md`, `plan.md`, `howto/` (build and run, direct access, monitoring, secure access, multiple cards), `spec/` (ring protocol, SBOX registers), `decisions/` (ADRs), `research/`, `results/` (dated measurements). |
| `tools/` | C helpers compiled with `tcc` or `gcc`: the two layout cross-checks (ring transport, offload protocol), a boot-path bisection stub, and the AVX-512 conformance, demo and vector-generation programs. |
| `vendor/` | Git-ignored. Reference material fetched by `scripts/fetch-vendor.sh` (MPSS 3.8.6 archives, Intel's k1om kernel tree, PDFs). Never committed, never linked into builds. |

## Where to start

1. [`docs/hardware.md`](docs/hardware.md): what the card is and how this host sees it.
2. [`docs/reproducibility.md`](docs/reproducibility.md): the walkthrough from a fresh Arch Linux install to a booted card with SSH, in order, with durations.
3. [`docs/howto/direct-access.md`](docs/howto/direct-access.md) and [`docs/howto/build-and-run.md`](docs/howto/build-and-run.md): daily use; [`docs/howto/multiple-cards.md`](docs/howto/multiple-cards.md) when there is more than one card (up to 16, each addressed by an index).
4. [`docs/spec/ring-protocol.md`](docs/spec/ring-protocol.md): the one transport everything rides on.
5. [`docs/decisions/`](docs/decisions/README.md) and [`docs/research/`](docs/research/README.md): why it is built this way.

## Quick start on Arch Linux

One command does the whole thing, from a fresh install to a running card:

```sh
git clone https://github.com/Lasimeri/Intel-Phi-3120A.git "Intel Phi 3120A"
cd "Intel Phi 3120A"
export PHI_DISK=/var/lib/phi/disk.img   # where the card's persistent disk goes
scripts/phi-install.sh
```

It runs the seventeen stages of `docs/reproducibility.md` in order, checks
whether each one is already done before doing it, and resumes where it
stopped if anything fails. It asks for `sudo` once, up front, for the two
stages that need it; nothing after that needs root. Expect a couple of
hours the first time, almost all of it the two LLVM builds.

```sh
scripts/phi-install.sh status     # what is done, what is not, what it makes
scripts/phi-install.sh --dry-run  # what it would do
scripts/phi-install.sh --full     # also build clang to run on the card
```

Then, from any shell, without `sudo`:

```sh
phi cards             # every card: index, address, link, state
phi status            # one line per card; phi -c N status for one in full
phi up                # boot card 0 (the autoboot stage does this at host boot); phi up all
phi run nproc         # 228; stdin, stdout, stderr and the exit status relayed
phi -c 1 sh           # an interactive login shell on card 1
phi top               # the live viewer, every card in its own block (or just: phitop)
phi ssh-config --apply  # ssh phi, ssh phi1, ... through ~/.ssh/config
phi down all          # stop; unmounts each card's disk first
```

With more than one card, `-c N` (or `PHI_CARD=N`) names the card for
every command; card 0 is the default and keeps every single-card path
(`docs/howto/multiple-cards.md`).

`scripts/phi-install.md` documents the stages, `scripts/phi.md` the rest of
the commands. To do it by hand instead, or to understand what any stage is
doing, `docs/reproducibility.md` is the walkthrough with durations.
## Running AVX-512 on a host that has none

This host is a Ryzen 7 5800X: AVX2 and FMA3, and no AVX-512 at all. **The
card executes AVX-512 on its behalf.** A kernel's AVX-512 machine code is
rewritten into the card's own instruction set ahead of time (a two-bit
change per instruction for the arithmetic that maps 1:1), a resident worker
on the card runs it across the 57 vector units, and every result lane is
the bit AVX-512 hardware would have produced.

```sh
phi vpu poly --n 16777216 --threads 57         # deploy, start, run, verify, on card 0
phi -c 1 vpu poly --n 16777216 --threads 57    # the same on card 1
```

| piece | what it does | where it runs |
| --- | --- | --- |
| `host/crates/avx512-xlate` | rewrites AVX-512 into the card's own instruction set, ahead of time | the host, once |
| `card/vpu` | the resident worker: doorbell in host memory, DMA in, 57 pinned vector threads, DMA out | the card |
| `host/crates/phi-vpu` | fills the window, rings the doorbell, checks every lane against this host's FMA hardware | the host |
| `host/crates/phi512` | the fallback: catches the `SIGILL` and performs the instruction in software, rewriting the site so it never faults again | the host |

Measured 2026-09-22: **316 GFLOP/s on the vector units** for a
16-million-element kernel, bit-identical to the host's FMA3 throughout;
0.3 ms of compute per million elements once the thread pool is warm. The
transport is now the bound: the host-memory block path serves one 512 KiB
record at a time, so moving that kernel's data costs about 70 ms against
3 ms of compute
([`docs/results/2026-09-22-avx512-coprocessor.md`](docs/results/2026-09-22-avx512-coprocessor.md)).

The comparison that matters for a host with no AVX-512 is against running
the same code in software, which is the only other way to run it at all.
The same 65536-element kernel takes 26.5 ms under `phi512` and 4.0 ms on
the card, transport included.

The software path is also what makes unmodified binaries work at all:

```sh
scripts/phi512-install.sh --system     # every process, automatically
phi512 ./a-program-built-for-avx512    # or one at a time
```

The program is not recompiled, not patched on disk, and not aware of any of
this. Conformance is checked by compiling one source twice, native and
AVX-512, and diffing the output (`scripts/phi512-check.sh`), and
`scripts/phi512-ground.sh` requires the host's FMA3 hardware, the software
path and the card to agree to the bit on one program.
[`docs/research/avx512-on-knc.md`](docs/research/avx512-on-knc.md) is the
precision audit that gates all of it;
[`docs/research/avx512-transparency.md`](docs/research/avx512-transparency.md)
has what each stage of the software path costs.

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
