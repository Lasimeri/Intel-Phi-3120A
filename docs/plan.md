# Plan

Phases are ordered by dependency. Each has an exit criterion that is a
measurement, not an opinion. The State column names the dated record in
`docs/results/` that proves it; deviations from the deliverable as first
written are stated, not hidden.

| Phase | Deliverable | Exit criterion | State |
| --- | --- | --- | --- |
| **P0 Foundations** | Repository, research record, decisions, specs, Arch setup scripts, host workspace compiling with tests, `phi-isa-audit` working on host binaries | `make check` passes on a machine without the card; `phi-isa-audit /usr/bin/ls` reports the expected CMOV/SSE hits | Done 2026-09-13 |
| **P1 First contact** | `phictl info/postcode/spad/reset` against the real card over VFIO | After `phictl reset`, POST code returns to `"12"` and `SPAD2` bit 0 reads 1; recorded in `docs/results/` with date and command | Done 2026-09-13: `docs/results/2026-09-13-reset-2.md` |
| **P2 Card toolchain** | Patched LLVM (x87 return, CMOV off), `knc-cc` wrapper, Rust target JSON with `build-std`, musl built with it | A static `hello` in C and in Rust link, and `phi-isa-audit` reports zero illegal instructions in both | Done 2026-09-13: C half `docs/results/2026-09-13-p2-c-toolchain.md`, Rust half `docs/results/2026-09-13-p2-rust.md` (distro rustc plus the patched LLVM dylib, no nightly: ADR 0007) |
| **P3 Kernel to early console** | KNC platform layer (SFI parser, I/O APIC at 64-bit address, no legacy devices, LAPIC timer calibration from SBOX, PGE cleared, barrier and `cpu_relax` variants), kernel built with `LLVM=1`, early console writing the ring; `phictl boot` and `phictl console` | The first `printk` lines of a mainline kernel appear in `phictl console`, POST code shows the kernel's own progress values | Done 2026-09-14: `docs/results/2026-09-14-first-boot.md` (full boot with `nosmp`, printk through the ring, init started; the host resets on the way are in `2026-09-13-p3-kernel-build.md`) |
| **P4 Full kernel and initramfs** | SMP bring-up of all 228 threads, tty and netdev over the ring, busybox and an `init` in a musl initramfs, host TAP bridging | `ping` the card from the host; `nproc` on the card prints 228 | Done 2026-09-14: `docs/results/2026-09-14-p4-tty-smp.md` (tty patch 0016; all 228 CPUs online with serial bring-up, patches 0017 to 0020, tests 8 and 9) and `2026-09-14-p5-net-rpc.md` (`phi0`, kernel patch 0021, bridged by `phictl boot --net`, ping 2.6 ms). Deviations: the ring drivers are kernel patches, not a `phinet` module (`card/drivers/phinet/` keeps only the C header); `init` is a busybox shell script (`card/initramfs/init`), not Rust |
| **P5 SSH** | dropbear on the card, key provisioning, host `~/.ssh/config` entry | `ssh phi uname -a` prints the mainline version string | Done 2026-09-15: dropbear 2025.88 static, started by init (test 13); root's `authorized_keys` baked into the initramfs from `~/.ssh/phi_ed25519.pub` at build time (not provisioned by `phictl`); root-free SSH through `phictl boot --forward 2222:22` (`ssh phi-fwd 'nproc; uptime'`, `scp`, two sessions at once: test 16 in `2026-09-14-p5-net-rpc.md`). The direct-access tool (`phictl boot --serve` + `exec/put/get/status`, ring channel 3, `/dev/phirpc`, Rust `phi-agent`; ADR 0008, `docs/howto/direct-access.md`) landed in the same phase |
| **P6 Daemon and interrupts** | MSI-X via VFIO eventfds, SBOX ICR doorbells, a service that boots the card and reboots it on hang, `phictl status` | Card survives a service restart; doorbell latency measured and recorded | Partly done. Done: the daemon is the `phictl boot --serve` process, serving several clients with one session holder (ADR 0009, 2026-09-17); `phictl status`, `sensors`, `traffic`; autoboot as the user unit `phi.service` at login with `Restart=on-failure` (`scripts/phi-autoboot.sh`, `2026-09-16-autoboot.md`, `systemctl --user restart phi.service` brings the card back). Boot at host boot rather than at login done 2026-09-19 (lingering plus a VFIO wait in the unit, `2026-09-19-boot-without-login.md`). Not done: interrupts of any kind (every channel polls at 1 kHz or faster), detection of a hung card, doorbell latency |
| **P7 C on the card** | Native clang, musl headers, `make`, `binutils`; pthread test that uses all threads | A pthread program compiled *on the card* runs on 228 threads and passes `phi-isa-audit` | Done 2026-09-14: `docs/results/2026-09-14-p7-native-clang.md` (clang 22 on the card, pthreads program compiled and run on 228 threads, audit clean; the LLVM tools stand in for binutils; no `make` on the card yet) |
| **P8 gcc and tcc on the card** | gcc with the knc64-x87 backend amendments, tcc with an x87 float backend, both native | Both compile and run a shared C test suite on the card | Not started; design notes in `card/userland/components/gcc.md` and `tcc.md` |
| **P9 Python and JavaScript** | CPython 3.13+ with the standard library including `ctypes` (patched libffi), `ssl`, `sqlite3`; QuickJS | `python3 -m test` subset passes; `qjs` runs the test262-lite subset shipped with QuickJS | Partly done: CPython 3.14.7 cross-built as one static interpreter with the modules the sysroot supports (`card/userland/components/cpython.sh`, 2026-09-14, used by the sibling glances-phi project), not in the default image, exit test not run; QuickJS not started (`quickjs.md`) |
| **P10 Stretch** | DMA engine for bulk transfer, vector ISA access, thermal and power readout, `perf` via the mainline KNC PMU driver | Each recorded with a measurement | Vector ISA done 2026-09-15 (`host/crates/knc-mvex`, patch 0024, Mandelbrot on the VPU 1.47x the host's AVX2: `2026-09-15-vpu.md`). DMA engine done 2026-09-16 (host-owned channel, status-descriptor completion, 3.58 GB/s memory to memory, the disk at 846 MB/s: `2026-09-16-dma.md`). Thermal readout done 2026-09-17 (hwmon `knc`, patch 0027, `phictl sensors`) and PMU counters through `perf_event_open` (`phiperf`): `2026-09-16-sensors.md`. Power readout open (the SMC holds it; not driven) |
| **Beyond the plan** | Persistent storage, host memory for the card, a remote resource viewer | Each recorded with a measurement | Storage done 2026-09-16 (`/dev/phiblk0` over ring channel 4 from a host disk image, mounted on `/data`: `2026-09-16-storage.md`). Host memory done 2026-09-16 (patch 0026: `/dev/phiblk1` as swap, `/dev/phihost` shared window). Viewer done 2026-09-17 (`phitop`, ADR 0009: `2026-09-17-phitop.md`) |

Next in order: `make` on the card, QuickJS (P9), OpenSSL for SSH clients on
the card, gcc and tcc (P8), interrupts (P6).

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
| Bootstrap `boot_params`/SFI contents differ from the SSDG description | Retired 2026-09-14 | The first boot found the SFI SYST at `0xef180` with CPUS, APIC and MMAP tables and an e820 in `boot_params`; both are consumed (`2026-09-14-first-boot.md`) |
| APs parked by the bootstrap do not respond to standard INIT/SIPI | Retired 2026-09-14 | Standard INIT/SIPI works with the expanded ICR destination field (patch 0017) and serial bring-up (patch 0020); parallel bring-up starves on the trampoline lock (`2026-09-14-p4-tty-smp.md`, tests 5 to 9) |
| User-space `LDMXCSR` with an image lacking MXCSR.DUE (bit 21) faults like `FXRSTOR` does (musl `fesetenv(FE_DFL_ENV)` loads `0x1f80`) | Open, low | Kernel side done in patch 0018 (`2026-09-14-p4-tty-smp.md`); musl's default floating-point environment with DUE set is still to do; nothing built so far has called `fesetenv` |
| LLVM patch scope grows | Retired 2026-09-13 | Nine patches (`toolchain/llvm/patches/`), bounded by `phi-isa-audit` on every product |
| Multi-byte NOP (`0F 1F`) faults on KNC | Avoided, not measured | Every product is assembled with single-byte-safe padding (`-nopl`, LLVM patch 0003, kernel patch 0006); whether `0F 1F` faults has not been tested on the card |
| Bootstrap flash too old for any image download | Retired 2026-09-14 | The card boots this project's kernel; the oracle VM was never needed |
| Host resets from aperture traffic | Retired 2026-09-14 | Cause: writes during GDDR retraining after the VFIO open reset; the loader waits for POST `"12"` (`2026-09-13-p3-kernel-build.md`) |
| DMA completion reported before the data is visible | Retired 2026-09-16 | Completion by status descriptor, never by the tail pointer (`2026-09-16-dma.md`) |
| A direct (`O_DIRECT`) read on `/dev/phiblk0` stalled once in state D; not reproduced in two attempts | Open, low | What to capture next time is listed in `2026-09-17-phitop.md`; since kernel patch 0028 the driver logs a waiting request every 30 s with both rings' indices; the request is not failed (its pages belong to the host's DMA engine), so a stall still needs a restart |
| A signal handler that uses the VPU clobbers the interrupted code's vector registers (signal frames carry the FXSAVE image only) | Open, low | Nothing on the card uses the VPU from a signal handler; documented in `2026-09-15-vpu.md` |
| The SBOX I/O APIC reads version 0 | Open, low | Left unregistered (patch 0023); nothing routes through it while every channel polls |
| Host power: 300 W card plus 3090 Ti exceeds the UPS | Certain if both loaded | Operational rule in `docs/hardware.md` |
| Time and effort | Certain | Phases are small; each one leaves a working, documented state |

## Non-goals

Intel's offload model (COI, MYO, `#pragma offload`), SCIF, OpenCL, MPI over
PCIe, flash updates, Windows hosts, cards other than the 3120A in this
machine (other KNC SKUs should work but are not tested).
