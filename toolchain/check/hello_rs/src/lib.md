# toolchain/check/hello_rs

The Rust half of the phase P2 exit test. Built as a `staticlib` for the
custom target with `build-std` (see `../../rust/build-std.sh`), then
linked into `hello.c` by `../run.sh` when `libhello_rs.a` exists, where
`main` prints `rust_hypot=5` and the C-side weak stub is overridden.

What it proves: `f64` crosses the C/Rust boundary in both directions with
the same convention (stack in, `ST0` out), `std` compiles and links for the
target (formatting, `Vec`, `String`), and a 64-bit conditional compiles to
a branch. The whole binary must pass `phi-isa-audit` clean.

`.cargo/config.toml` selects the target spec and `knc-cc` as the linker
and turns on `build-std`. `RUSTC_BOOTSTRAP=1` in the driver script lets
Arch's stable `rustc` accept the `-Z` flags; the alternative is a nightly
toolchain from `rustup`.

`cargo test` here runs on the host with the host target and only checks
the arithmetic; the target build is exercised by `run.sh`.
