# card/

Everything that runs on the coprocessor. Nothing here builds with the host's
default compiler; all of it is compiled by the toolchain in `toolchain/`
(a patched LLVM producing the knc64-x87 ABI, `docs/decisions/0002`) and
verified with `phi-isa-audit`.

| Directory | Contents |
| --- | --- |
| `kernel/` | The 28-patch series against v7.2.3 (platform layer, SFI reader, ring console, tty, netdev, rpc, block devices, hwmon, instruction-set fixes), the Kconfig fragment, and `build.sh` |
| `initramfs/` | `init` (a busybox shell script, PID 1) and `build.sh`, which packs the root filesystem |
| `agent/` | `phi-agent`, Rust, the card end of `phictl exec/put/get/status` over `/dev/phirpc`, and the telemetry sampler `phitop` reads |
| `userland/` | Per-component build scripts and notes: busybox, dropbear, zlib, ncurses, CPython, clang packaging, and notes for gcc, tcc, QuickJS |
| `examples/` | Benchmarks and probes compiled on the card: Mandelbrot (scalar and VPU), BBP pi, the VPU probe and state test, `phiperf` |
| `drivers/phinet/` | One C header, `include/phi_ring.h`, the C mirror of the ring layout. No module is built; the name is historical (see its README) |

## Ground rules

- Kernel patches are C (GPL-2.0-only), because that is what they amend.
  Everything the project writes for the card that is not a kernel patch is
  Rust (`phi-agent`) or a shell script (`init`). Third-party C projects are
  built from upstream with small patches kept here.
- Every binary destined for the card passes `phi-isa-audit` with zero
  illegal instructions before it is packed. `card/initramfs/build.sh`
  enforces it on busybox and on every ELF file under `extra/`.
- The card boots entirely from the initramfs. Since 2026-09-16 it also
  mounts a host-served disk image on `/data`
  (`docs/results/2026-09-16-storage.md`), which is where the on-card
  toolchain lives; the initramfs stays small and is regenerated on the
  host.
