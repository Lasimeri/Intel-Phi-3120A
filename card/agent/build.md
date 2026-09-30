# card/agent/build.sh

Builds `phi-agent` for the card from the assembly sources: `as --64` for
each of `agent.S`, `wire.S`, `exec.S`, `xfer.S`, `stat.S`, `text.S`,
`errno.S` (with `-I` here for `defs.inc`), then `ld -static -nostdlib -e
_start -z noexecstack -s`. GNU binutils is the only requirement: no LLVM,
no rustc, no C library. The result is about 20 KiB.

Steps: assemble and link, audit with `phi-isa-audit` (`make build` first;
the audit must be clean, and a hit fails the build), then move the binary
to `~/.cache/intel-phi-3120a-build/userland/agent/phi-agent` (the same
place the Rust agent went, `card/userland/build/agent/` through the build
symlink), from where `card/initramfs/build.sh` installs it as
`/bin/phi-agent`. Cards run it from their next boot with that image.

`build.sh --out DIR` only assembles and links into `DIR/phi-agent`; the
host tests (`host/crates/phi-rpc/tests/agent.rs`) build it that way on
every `make check`.

Until 2026-09-29 this script built the Rust agent (cargo with the card
target, `build-std`, the patched LLVM dylib, `RUSTC_BOOTSTRAP=1`); `agent.md`
says why that changed.
