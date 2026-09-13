# host/

Rust workspace for everything that runs on the host. Build with `cargo build`
from this directory or `make build` from the repository root.

| Crate | Kind | Purpose |
| --- | --- | --- |
| `phi-regs` | library, no I/O | SBOX/DBOX register offsets, scratchpad decoding, POST codes, bzImage setup-header offsets, card memory constants. Every constant cites its source. |
| `phi-vfio` | library | Minimal VFIO (type1 container, group, device) bindings written against `/usr/include/linux/vfio.h`. Region mmap with volatile access, config-space access, DMA map, IRQ eventfds. |
| `phi-hw` | library | The card as an object: open by BDF, read POST code and scratchpads, reset, load a kernel image and initramfs into the aperture, send the boot interrupt. |
| `phi-ring` | library, no I/O | Host side of the ring protocol in `docs/spec/ring-protocol.md`, over an abstract memory trait so it is unit-tested without hardware. |
| `phictl` | binary | Command-line front end: `info`, `postcode`, `spad`, `regs`, `reset`, `boot`, `console`. |
| `phi-isa-audit` | binary | Disassembles any x86-64 ELF and reports instructions Knights Corner cannot execute. Used on every card binary and on the kernel. |

Hardware-touching code paths are only exercised when `PHI_BDF` is set or a
BDF is passed on the command line; `cargo test` never opens the device.
