# host/

Rust workspace for everything that runs on the host. Build with `cargo build`
from this directory or `make build` from the repository root. Nine crates.

| Crate | Kind | Purpose |
| --- | --- | --- |
| `phi-regs` | library, no I/O | SBOX/DBOX register offsets, scratchpad decoding, POST codes, bzImage setup-header offsets, card memory constants. Every constant cites its source. |
| `phi-vfio` | library | Minimal VFIO (type1v2 container, group, device) bindings written against `/usr/include/linux/vfio.h`. Region mmap with volatile access, config-space access, DMA map, IRQ eventfds, and the PCIe traffic counters `phictl traffic` and `phitop` report. |
| `phi-ring` | library, no I/O | Host side of the ring protocol in `docs/spec/ring-protocol.md`, over an abstract memory trait so it is unit-tested without hardware. |
| `phi-hw` | library | The card as an object: open by BDF, read POST code and scratchpads, reset, load a kernel image and initramfs into the aperture, send the boot interrupt, and drive the eight-channel SBOX DMA engine (`dma.rs`) that moves bulk data at full link rate. |
| `phi-rpc` | library, no I/O | The framing shared by `phictl` and `phi-agent`: request and response records on the rpc channel, used by exec, put, get, status and the telemetry sideband. |
| `phictl` | binary | The control tool. Everything below. |
| `phitop` | binary | Live viewer: per-core load for all 57 cores x 4 threads, die temperatures, memory, PCIe rates, and the card's process table. Talks to a running `phictl boot --serve` daemon, never to the card directly. |
| `phi-isa-audit` | binary | Disassembles any x86-64 ELF and reports instructions Knights Corner cannot execute. Used on every card binary and on the kernel. |
| `knc-mvex` | library + binary | The MVEX encoder and `knc-mvex-gen`, which generates the vector save/restore sequences kernel patch 0024 needs (KNC's zmm registers are not covered by FXSAVE). |

## phictl commands

| Command | What it does |
| --- | --- |
| `info` | PCI identity, POST code, scratchpads, bootstrap state |
| `postcode` | The POST code; `--watch` prints every change |
| `spad` | One scratchpad (0..15) or all of them |
| `regs` | The named SBOX registers |
| `reset` | Reset to the bootstrap and wait for ready |
| `boot` | Load a kernel and initramfs and start the card |
| `console` | Tail the console ring, forward stdin to it |
| `exec`, `put`, `get`, `status` | Drive a running card through the control socket: run a command with stdin/stdout/stderr and exit status relayed, copy files in and out, check the agent answers |
| `sensors` | Temperatures, core voltage and clock, read from the SBOX through the daemon |
| `traffic` | Bytes moved over PCIe since the daemon started, by path (DMA engine, aperture) and direction |
| `peek`, `poke`, `fill` | Single aperture read, single aperture write, and a timed bulk write. Diagnostics for the aperture in isolation |

`phictl boot` is where the options are:

| Option | Effect |
| --- | --- |
| `--kernel`, `--initrd`, `--cmdline` | What to load. Default cmdline `earlyprintk=phiring,keep loglevel=8` |
| `--ring-base`, `--ring-size` | Card physical base and size of the ring region. Defaults 256 MiB and 16 MiB |
| `--net NAME`, `--net-addr` | Bridge the ring network channel to a host TAP device. Needs root |
| `--forward 2222:22` | Forward a localhost port to the card through a userspace network stack on the ring. No root; this is how `ssh phi-fwd` works |
| `--serve [PATH]` | Also serve the control socket, which is what `exec`, `put`, `get`, `status`, `sensors`, `traffic` and `phitop` connect to |
| `--owner UID` | Who may use that socket (default `SUDO_UID`, else root) |
| `--disk IMG` | Serve a disk image as `/dev/phiblk0`; the card mounts it on `/data` |
| `--host-mem SIZE` | Pin that much host RAM and serve it as `/dev/phiblk1` (swap) and `/dev/phihost` (direct mapping) |
| `--no-dma` | Serve the disk through aperture copies instead of the DMA engine |
| `--watch ADDR`, `--load-only`, `--no-console`, `--raw-cmdline` | Boot diagnostics |

Run `phictl <command> --help` for the authoritative list; the tables above
are a map, not a substitute.

## Testing

Hardware-touching code paths are only exercised when `PHI_BDF` is set or a
BDF is passed on the command line; `cargo test` never opens the device.
The ring, the layout, the POST decoding, the boot-parameter writer, the
RPC framing and `phitop`'s model are all unit-tested against in-memory
buffers.
