# Documentation index

| Document | Contents |
| --- | --- |
| [plan.md](plan.md) | Phased roadmap with exit criteria, current phase, risk register |
| [hardware.md](hardware.md) | The card, the host it sits in, measured PCIe state, power budget |
| [reproducibility.md](reproducibility.md) | End-to-end procedure on a fresh Arch Linux install |
| [research/](research/README.md) | Everything learned before design: ISA deletions, OS limitations, memory map, boot protocol, ABI and toolchain, prior art, runtime feasibility, sources |
| [decisions/](decisions/README.md) | Architecture decision records with alternatives considered |
| [spec/sbox-registers.md](spec/sbox-registers.md) | The SBOX/DBOX register subset this project touches, with sources |
| [spec/ring-protocol.md](spec/ring-protocol.md) | The host/card shared-memory transport (console and network) |

Reading order for a newcomer: `hardware.md`, then `research/isa-deletions.md`
and `research/os-limitations.md`, then `decisions/`, then `plan.md`.
