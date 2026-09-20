# Documentation index

| Document | Contents |
| --- | --- |
| [reproducibility.md](reproducibility.md) | The fresh-clone walkthrough: packages, verification, binding, host tools, toolchain, kernel, initramfs, disk image, first boot, SSH, autoboot, monitoring, with durations |
| [hardware.md](hardware.md) | The card, the host it sits in, measured PCIe state and transfer rates, power budget; every row with its source |
| [plan.md](plan.md) | Phased roadmap with exit criteria and the record that proves each, risk register, non-goals |
| [howto/build-and-run.md](howto/build-and-run.md) | Compile for the card on the host or on the card, move files, run programs |
| [howto/direct-access.md](howto/direct-access.md) | The control socket: `phictl boot --serve`, `exec`, `put`, `get`, `status`; the unprivileged scripts |
| [howto/monitoring.md](howto/monitoring.md) | `phitop`, `phictl sensors` and `traffic`, hwmon on the card, `phiperf`, the service's journal |
| [howto/secure-access.md](howto/secure-access.md) | Who can reach the card through which path, keys, what is not protected |
| [spec/ring-protocol.md](spec/ring-protocol.md) | The host/card shared-memory transport: region layout, channel kinds 1 to 5 (console, network, rpc, block, host memory), frames and records |
| [spec/sbox-registers.md](spec/sbox-registers.md) | The SBOX/DBOX register subset this project touches, with sources |
| [decisions/](decisions/README.md) | Architecture decision records (nine) with alternatives considered |
| [research/](research/README.md) | Everything learned before design: ISA deletions, OS limitations, memory map, boot protocol, ABI and toolchain, prior art, runtime feasibility, sources |
| [results/](#results) | Dated records of every measurement made on the card, listed below |

Reading order for a newcomer: `hardware.md`, `reproducibility.md`, the
howtos, `spec/ring-protocol.md`, then `research/isa-deletions.md`,
`research/os-limitations.md`, `decisions/` and `plan.md`.

## Terms

| Term | Meaning here |
| --- | --- |
| card | the Xeon Phi 3120A: 57 cores, 228 hardware threads, 6 GB GDDR5, its own Linux kernel |
| host | the PC it is plugged into; runs the Rust tools |
| daemon | the `phictl boot --serve` process: it holds the VFIO device, tails the console, serves the control socket, the disk, host memory and the SSH forwarder; `phi.service` runs it, at host boot when lingering is on and otherwise at the first login |
| agent | `phi-agent` on the card: answers the daemon's rpc frames (run a program, copy a file, report a sample) |
| aperture | PCIe BAR0, 16 GiB, through which the host reads and writes card memory directly (uncached, slow) |
| ring | a single-producer single-consumer byte ring in a reserved region of card memory; the transport of every channel |
| channel kinds | 1 console (tty), 2 network (`phi0`), 3 rpc (control socket), 4 block (`/dev/phiblk0`), 5 host memory (`/dev/phiblk1`) |
| SMPT | the card's system memory page table: 32 windows of 16 GiB through which the card (and its DMA engine) reach host memory |
| VPU | the card's 512-bit vector unit, MVEX-encoded, driven only by generated `.byte` sequences (`knc-mvex`) |
| POST code | a two-character progress code in a DBOX register, written by the bootstrap and then by the card kernel (`"12"` ready, `K0`..`K7` kernel, `KH` halted) |

## Results

Each file records the date, the host kernel and commit, the command, the
output and a reading. In date order:

| Record | Proves |
| --- | --- |
| [2026-09-13-first-contact.md](results/2026-09-13-first-contact.md) | VFIO path end to end; `SPAD2` decode matches Intel's driver |
| [2026-09-13-reset-1.md](results/2026-09-13-reset-1.md) | `RGCR` reset from userspace works |
| [2026-09-13-reset-2.md](results/2026-09-13-reset-2.md) | POST register is a live ASCII pair; the reset sequence and its timing (P1 exit) |
| [2026-09-13-p2-llvm.md](results/2026-09-13-p2-llvm.md) | The patched LLVM emits knc64-x87 code |
| [2026-09-13-p2-c-toolchain.md](results/2026-09-13-p2-c-toolchain.md) | C hello links against musl, audits clean, runs on the host (P2, C half) |
| [2026-09-13-p2-rust.md](results/2026-09-13-p2-rust.md) | Rust `std` for the card through the distro rustc and the patched dylib (P2, Rust half) |
| [2026-09-13-p3-kernel-build.md](results/2026-09-13-p3-kernel-build.md) | The kernel builds and audits clean; the host resets and their cause (aperture writes during GDDR training) |
| [2026-09-14-first-boot.md](results/2026-09-14-first-boot.md) | Mainline Linux boots to init with `nosmp` (P3 exit) |
| [2026-09-14-p4-tty-smp.md](results/2026-09-14-p4-tty-smp.md) | The ring tty, MXCSR.DUE, a shell, all 228 CPUs online with serial bring-up (P4) |
| [2026-09-14-p5-net-rpc.md](results/2026-09-14-p5-net-rpc.md) | `phi0` over the ring, dropbear, the direct-access tool, root-free SSH through the forwarder (P5) |
| [2026-09-14-p7-native-clang.md](results/2026-09-14-p7-native-clang.md) | clang 22 compiles and runs programs on the card; the unprivileged scripts (P7) |
| [2026-09-15-mandelbrot.md](results/2026-09-15-mandelbrot.md) | x87 CPU benchmark: the whole card is 0.25 of the host's 16 threads; per-CPU load verified |
| [2026-09-15-pi-bbp.md](results/2026-09-15-pi-bbp.md) | Integer benchmark: 0.14 of the host; the divider is not the bulk of the cost |
| [2026-09-15-vpu.md](results/2026-09-15-vpu.md) | The vector unit: encoder, kernel state save/restore, 1.47x the host's AVX2 on the iteration pass |
| [2026-09-16-storage.md](results/2026-09-16-storage.md) | `/dev/phiblk0` from a host disk image; the torn-index bug; aperture-bound rates |
| [2026-09-16-dma.md](results/2026-09-16-dma.md) | The DMA engine: coherence settled, completion by status descriptor, 846 MB/s disk reads |
| [2026-09-16-autoboot.md](results/2026-09-16-autoboot.md) | `phi.service` at login (superseded by boot-without-login, below); host key pinning and file modes |
| [2026-09-16-sensors.md](results/2026-09-16-sensors.md) | hwmon on the card, `phictl sensors`, the core clock from `COREFREQ`, PMU counters with `phiperf` |
| [2026-09-17-phitop.md](results/2026-09-17-phitop.md) | The viewer under load; PCIe rates against the card's own counters; the one direct-read stall |
| [2026-09-19-zstd.md](results/2026-09-19-zstd.md) | zstd 1.5.7 against the host: 0.03 to 0.07x, why cheap-per-byte algorithms lose here, and a CMOV the patched LLVM still emits |
| [2026-09-19-xz.md](results/2026-09-19-xz.md) | xz 5.8.3 on the card against the host: 0.22 to 0.26x, block count is the real thread limit, and a portability bug in liblzma's range decoder |
| [2026-09-19-boot-without-login.md](results/2026-09-19-boot-without-login.md) | The card from host boot with nobody logged in: lingering instead of a system unit, what that costs, and the VFIO race it opened |
| [2026-09-19-card-os.md](results/2026-09-19-card-os.md) | The card as an ordinary Linux system: FHS layout, persistent `/etc` and `/var`, 6 GiB of host RAM as swap verified under pressure, a shutdown that leaves the filesystem clean, and a cold host reboot that reaches a running card unattended |

Images: `results/images/`.
