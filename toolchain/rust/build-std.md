# toolchain/rust/build-std.sh

Compiles `toolchain/check/hello_rs` for `x86_64-knc-linux-musl` without
building rustc:

1. `LD_LIBRARY_PATH` points at the patched `libLLVM.so.22.1` from the
   `dylib` variant of `toolchain/llvm/build.sh`; the distro `rustc`
   (which links that soname) picks it up. The script prints the resolved
   path so the substitution is visible.
2. `RUSTC_BOOTSTRAP=1` allows `-Zbuild-std` and `-Zjson-target-spec` (a
   rustc 1.98 gate on custom target files) on the stable compiler.
3. `cargo build --release` with the crate's `.cargo/config.toml`: custom
   target JSON, `build-std`, `knc-cc` as linker.
4. `phi-isa-audit` on the resulting `staticlib` (no `--allow-suspect`: the
   `-nopl` feature in the target spec and the patched assembler must have
   removed multi-byte NOPs).

If rustc crashes at load with an undefined symbol, the dylib's options
diverged from Arch's `llvm-libs`; compare `build.md`'s list with the
current PKGBUILD.
