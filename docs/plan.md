# Plan

Phases are ordered by dependency. Each has an exit criterion that is a
measurement, not an opinion. "Now" marks the current phase.

| Phase | Deliverable | Exit criterion | State |
| --- | --- | --- | --- |
| **P0 Foundations** | Repository, research record, decisions, specs, Arch setup scripts, host workspace compiling with tests, `phi-isa-audit` working on host binaries | `make check` passes on a machine without the card; `phi-isa-audit /usr/bin/ls` reports the expected CMOV/SSE hits | Done 2026-09-13 |
| **P1 First contact** | `phictl info/postcode/spad/reset` against the real card over VFIO | After `phictl reset`, POST code returns to 0x12 and `SPAD2` bit 0 reads 1; recorded in `docs/results/` with date and command | Done 2026-09-13: `docs/results/2026-09-13-reset-2.md` |
| **P2 Card toolchain** | Patched LLVM (x87 return, CMOV off), `knc-cc` wrapper, Rust target JSON with `build-std`, musl built with it | A static `hello` in C and in Rust link, and `phi-isa-audit` reports zero illegal instructions in both | Done 2026-09-13: C half `docs/results/2026-09-13-p2-c-toolchain.md`, Rust half `docs/results/2026-09-13-p2-rust.md` |
| **P3 Kernel to early console** | KNC platform layer (SFI parser, I/O APIC at 64-bit address, no legacy devices, LAPIC timer calibration from SBOX, PGE cleared, barrier and `cpu_relax` variants), kernel built with `LLVM=1`, early console writing the ring; `phictl boot` and `phictl console` | The first `printk` lines of a mainline kernel appear in `phictl console`, POST code shows the kernel's own progress values | Done 2026-09-14: `docs/results/2026-09-14-first-boot.md` (full boot with nosmp, printk through the ring, init started) |
| **P4 Full kernel and initramfs** | SMP bring-up of all 228 threads, `phinet` module (tty + netdev over the ring), busybox and a Rust `init` in a musl initramfs, host TAP bridging | `ping` the card from the host; `nproc` on the card prints 228 | In progress 2026-09-14: `docs/results/2026-09-14-p4-tty-smp.md` (busybox shell on `ttyPHI0`, input and output; all 228 CPUs online with serial bring-up, patches 0017 to 0020; remaining: `phinet` netdev over the ring, host TAP bridging, Rust `init`) |
| **P5 SSH** | dropbear on the card, key provisioning by `phictl`, host `~/.ssh/config` entry | `ssh phi uname -a` prints the mainline version string | In progress 2026-09-14: `phi0` over the ring (kernel patch 0021 + `phictl boot --net`), dropbear 2025.88 static in the initramfs with host keys and the user's public key (post-quantum KEMs off: inline-asm `cmov`); first SSH login pending |
| **P6 Daemon and interrupts** | MSI-X via VFIO eventfds, SBOX ICR doorbells, `phid` systemd service that boots the card at host boot and reboots it on hang, `phictl status` | Card survives `systemctl restart phid`; doorbell latency measured and recorded | |
| **P7 C on the card** | Native clang, musl headers, `make`, `binutils`; pthread test that uses all threads | A pthread program compiled *on the card* runs on 228 threads and passes `phi-isa-audit` | |
| **P8 gcc and tcc on the card** | gcc with the knc64-x87 backend amendments, tcc with an x87 float backend, both native | Both compile and run a shared C test suite on the card | |
| **P9 Python and JavaScript** | CPython 3.13+ with the standard library including `ctypes` (patched libffi), `ssl`, `sqlite3`; QuickJS | `python3 -m test` subset passes; `qjs` runs the test262-lite subset shipped with QuickJS | |
| **P10 Stretch** | DMA engine for bulk transfer, vector ISA access via inline assembly, thermal and power readout, `perf` via the mainline KNC PMU driver | Each recorded with a measurement | |

## Dependencies between phases

```
P0 -> P1 -> P3 -> P4 -> P5 -> P6
       \      ^
        P2 --/   (P2 is needed before P3 can build the kernel)
P4 -> P7 -> P8
P7 -> P9
```

## Risk register

| Risk | Likelihood | Retirement |
| --- | --- | --- |
| Bootstrap `boot_params`/SFI contents differ from the SSDG description | Medium | P3: the early console prints `boot_params` and the SFI tables before anything else |
| APs parked by the bootstrap do not respond to standard INIT/SIPI | Medium | P4: compare with `intelmic.c`/`smpboot.c` in Intel's tree; fall back to their wake mechanism |
| User-space LDMXCSR with an image lacking MXCSR.DUE (bit 21) faults like FXRSTOR does (musl `fesetenv(FE_DFL_ENV)` loads `0x1f80`) | Low | P4: musl default floating-point environment with DUE set; kernel side done in patch 0018 (`docs/results/2026-09-14-p4-tty-smp.md`) |
| LLVM patch scope grows (clang frontend check, `canUseCMOV` assumptions) | Medium | P2: bounded by `phi-isa-audit`; scope is measured, not guessed |
| Multi-byte NOP (`0F 1F`) faults on KNC | Low | P2: microbenchmark on the card; if it faults, assemble with single-byte NOP padding |
| Bootstrap flash too old for any image download | Low | P1: POST code and `SPAD2` behavior; a CentOS 7 VM with MPSS 3.8.6 and the card passed through is the recovery oracle |
| Host power: 300 W card plus 3090 Ti exceeds the UPS | Certain if both loaded | Operational rule in `docs/hardware.md` |
| Time and effort | Certain | Phases are small; each one leaves a working, documented state |

## Non-goals

Intel's offload model (COI, MYO, `#pragma offload`), SCIF, OpenCL, MPI over
PCIe, flash updates, Windows hosts, cards other than the 3120A in this
machine (other KNC SKUs should work but are not tested).
