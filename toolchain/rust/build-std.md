# toolchain/rust/build-std.sh

Compiles `toolchain/check/hello_rs` for `x86_64-knc-linux-musl` without
building rustc (ADR 0007):

1. `LD_LIBRARY_PATH` points at the patched `libLLVM.so.22.1` from the
   `dylib` variant of `toolchain/llvm/build.sh`; the distro `rustc`
   (which links that soname) picks it up. The script prints the resolved
   path so the substitution is visible.
2. `RUSTC_BOOTSTRAP=1` allows `-Zbuild-std` and `-Zjson-target-spec` (a
   rustc 1.98 gate on custom target files) on the stable compiler.
3. `cargo build --release` with the crate's `.cargo/config.toml`: custom
   target JSON, `build-std = core, alloc, std, panic_abort`, `knc-cc` as
   linker. `core`, `alloc` and `std` compile in about twenty seconds.
4. `phi-isa-audit --ignore XSAVE` on the staticlib. The archive bundles all
   of `std`, linked or not; the one ignored hit is `xgetbv` in `std_detect`
   (behind a CPUID.OSXSAVE check, dropped by `--gc-sections` unless a
   program calls `is_x86_feature_detected!`). Linked executables are
   audited with no exceptions by `toolchain/check/run.sh`.

Expected noise: one "target feature `sse2` must be enabled" warning per
crate (`x86_64-knc-linux-musl.md` explains it).

Prerequisites: the `rust` and `rust-src` packages, the dylib LLVM variant,
the musl sysroot with compiler-rt, and `toolchain/libunwind/build.sh` (the
final link of any Rust program needs `libunwind.a`).

If rustc crashes at load with an undefined symbol, the dylib's options
diverged from Arch's `llvm-libs`; compare `build.md`'s list with the
current PKGBUILD.
