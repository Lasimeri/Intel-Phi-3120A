# card/

Everything that runs on the coprocessor. Nothing here builds with the host's
default compiler; all of it is compiled by the toolchain in `toolchain/`
(a patched LLVM producing the knc64-x87 ABI, `docs/decisions/0002`) and
verified with `phi-isa-audit`.

| Directory | Contents | Phase |
| --- | --- | --- |
| `kernel/` | The patch series against a pinned mainline tag (platform layer, SFI reader, ring console, instruction-set fixes), the Kconfig fragment, and `build.sh` | P3, P4 |
| `drivers/phinet/` | Out-of-tree module: console `tty` and `netdev` over the ring transport; the shared C header for the ring layout | P4 |
| `userland/` | Per-component build notes: musl, busybox, dropbear, Rust `init`, clang, gcc, tcc, QuickJS, CPython | P4 to P9 |
| `initramfs/` | How the initramfs is assembled and what `init` does | P4 |

## Ground rules

- Kernel and modules are C (GPL-2.0-only). Userland services are Rust
  where possible. Third-party C projects are built from upstream with small
  patches kept here.
- Every binary destined for the card passes `phi-isa-audit` with zero
  illegal instructions before it is packed. The initramfs script enforces
  it.
- The card has no persistent storage of its own that this project writes.
  Everything is in the initramfs, regenerated on the host.
