# Architecture decision records

| ADR | Decision |
| --- | --- |
| [0001](0001-userspace-vfio-host.md) | The host side is a Rust userspace program on VFIO, not a kernel module |
| [0002](0002-64bit-userland-x87-abi.md) | Card userland is 64-bit with the knc64-x87 ABI produced by a patched LLVM |
| [0003](0003-custom-ring-transport.md) | Console and network use a new shared-memory ring protocol, not MPSS protocols |
| [0004](0004-quickjs-not-bun.md) | QuickJS is the JavaScript runtime; Bun is excluded |
| [0005](0005-forward-port-mainline.md) | The card runs a mainline kernel with a KNC platform layer, not Intel's 2.6.38 tree |
| [0006](0006-musl-libc.md) | musl is the card libc |
| [0007](0007-rust-distro-rustc-patched-llvm.md) | Rust for the card is the distro rustc loading the patched LLVM; SSE removed by reverse implication, no rustc fork |

Format: Status, Context, Decision, Consequences, Alternatives rejected.
