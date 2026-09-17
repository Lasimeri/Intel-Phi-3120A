# phi-regs / lib.rs

Crate root. Declares the modules, lists the source abbreviations used in
every doc comment, and holds the PCI identity constants.

## Why a separate crate

Register offsets are the kind of fact that gets copied into three places
and then diverges. Putting them in a crate with `#![forbid(unsafe_code)]`
and no dependencies makes them trivially reviewable: the whole crate is a
list of numbers with citations and unit tests that pin the derived values
(`apicicr(7) == 0xAA08`, `spad(2) == 0xAB28`) and the decoders to words
measured on this card.

## Modules

| Module | Contents |
| --- | --- |
| `sbox` | SBOX/DBOX offsets, scratchpad and clock decoders, sensors, DMA engine registers and descriptors |
| `postcode` | The POST code register's ASCII-pair encoding and Intel's table plus this project's kernel marks |
| `bootparams` | The bzImage setup header fields the loader validates and patches |
| `memory` | Card physical map, SMPT window, ring region default |

## Sources

The PCI IDs are from the v5.9 `mic_x100.c` device table and from `lspci -nn`
on this host (`docs/hardware.md`). The subsystem decode comes from
linux-hardware.org's entry for `8086:225d:8086:3c98`. Register offsets
prefer the KNC-only header in Intel's k1om tree over the merged KNF/KNC
header in MPSS (see `sbox.md` for the one case where they disagree).
Everything under `vendor/` is reference only: read, cited, never copied.

## Testing

`cargo test -p phi-regs`. No hardware.
