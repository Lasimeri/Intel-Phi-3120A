# Intel Phi 3120A

A from-scratch software stack for one Intel Xeon Phi 3120A coprocessor
(Knights Corner, PCI `8086:225d`, subsystem `8086:3c98`), written in Rust
wherever Rust can run, with C only where the hardware or an upstream project
leaves no choice.

The end state is narrow on purpose:

1. The card boots a **current mainline Linux kernel** (not Intel's 2.6.38 uOS).
2. You `ssh` into it from the host.
3. You compile and run C on its 57 cores / 228 hardware threads with a native
   toolchain on the card: `gcc`, `tcc`, plus `python3` and a JavaScript runtime.

Intel's MPSS (Manycore Platform Software Stack) is **not** ported. It is used as
a reference for the hardware/software contract only. Everything the host runs
is new code in this repository.

## Why this is hard, in one table

| Constraint | Consequence |
| --- | --- |
| KNC deletes CMOV, PAUSE, MONITOR/MWAIT, CMPXCHG16B, FCMOV/FCOMI, SYSENTER, IN/OUT, PREFETCH, CLFLUSH, fences, and every MMX/SSE/AVX instruction | No stock compiler output runs. Every binary is audited (`phi-isa-audit`). The kernel and userland are built with a patched LLVM. |
| KNC has no 32-bit compatibility submode inside long mode (SSDG 4.2.3) | Userland must be 64-bit with a float ABI that does not use XMM registers. |
| No PIT, RTC, HPET, ACPI, legacy PIC; I/O APIC at a 64-bit address; CR4.PGE faults | The kernel gets a small KNC platform layer modeled on mainline's jailhouse guest support. |
| The on-card bootstrap enters the kernel's 32-bit entry with SFI tables | A minimal SFI parser returns to the kernel (mainline removed SFI in 5.12). |
| The host kernel (7.x) has no Xeon Phi driver | The host side is a Rust userspace daemon on VFIO. No kernel module on the host. |
| Bun requires SSE4.2 even in baseline builds | QuickJS is the JavaScript runtime. |

Full research record: [`docs/research/`](docs/research/README.md).
Decisions and their rationale: [`docs/decisions/`](docs/decisions/README.md).
The phased plan with exit criteria: [`docs/plan.md`](docs/plan.md).

## Repository map

| Path | What lives there |
| --- | --- |
| `host/` | Rust workspace. `phictl` (CLI), `phi-vfio` (VFIO device access), `phi-regs` (register map), `phi-hw` (reset/boot/image loading), `phi-ring` (host side of the host/card ring transport), `phi-isa-audit` (flags KNC-illegal instructions in any x86-64 ELF). |
| `card/` | Everything that runs on the card: kernel patch series and config, the KNC platform layer, the `phinet` transport driver, userland component notes, initramfs assembly. |
| `toolchain/` | How the card compilers are built: LLVM patch design, the Rust custom target, the `knc-cc` clang wrapper. |
| `scripts/` | Arch Linux setup, VFIO binding, card verification, reference-source fetching, documentation lint. |
| `docs/` | Research, specifications, decisions, plan, hardware and reproducibility notes. |
| `tools/` | Small C helpers compiled with `tcc` (layout checks against the C headers). |
| `vendor/` | Git-ignored. Reference material fetched by `scripts/fetch-vendor.sh` (MPSS 3.8.6 archives, Intel's k1om kernel tree, PDFs). Never committed, never linked into builds. |

## Status

See [`docs/plan.md`](docs/plan.md) for the phase table. Short version: research
is complete; the card answers over VFIO and survives a traced reset (P1); the
card toolchain (patched LLVM, musl, compiler-rt, libunwind, Rust `std`) builds
and its exit check passes on the host (P2); the kernel port (P3) is next.
Nothing has booted on the card yet. Every claim about hardware behavior in this repository
is labeled with its source: a document, a source tree, or a measurement on this
machine.

## Quick start on Arch Linux

```sh
git clone https://github.com/Lasimeri/Intel-Phi-3120A.git "Intel Phi 3120A"
cd "Intel Phi 3120A"
sudo scripts/setup-arch.sh        # packages, udev rule, memlock limit, vfio-pci
scripts/verify-card.sh            # confirms the card, link, BARs, IOMMU group
sudo scripts/bind-vfio.sh         # hands 2e:00.0 (or the detected BDF) to vfio-pci
make build                        # cargo build of the host workspace
./host/target/debug/phictl info   # first contact: POST code and scratchpads
```

Everything above is documented step by step in
[`docs/reproducibility.md`](docs/reproducibility.md).

## Conventions

Every source file has a sibling Markdown file with the same stem that explains
the context the code cannot: why the file exists, which document or hardware
observation it depends on, and how to test it. `scripts/check-docs.sh` enforces
this, along with the other rules in [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

Project code is MIT (see `LICENSE.md`). Material under `card/kernel/` derives
from Linux and is GPL-2.0-only. Intel documents and MPSS archives referenced by
this project are not redistributed here.
