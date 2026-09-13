# phi-regs / lib.rs

Crate root. Declares the modules and the PCI identity constants.

## Why a separate crate

Register offsets are the kind of fact that gets copied into three places
and then diverges. Putting them in a crate with `#![forbid(unsafe_code)]`
and no dependencies makes them trivially reviewable: the whole crate is a
list of numbers with citations and unit tests that pin the derived values
(`apicicr(7) == 0xAA08`, `spad(2) == 0xAB28`).

## Sources

The PCI IDs are from the v5.9 `mic_x100.c` device table and from `lspci -nn`
on this host (`docs/hardware.md`). The subsystem decode comes from
linux-hardware.org's entry for `8086:225d:8086:3c98`.

## Testing

`cargo test -p phi-regs`. No hardware.
