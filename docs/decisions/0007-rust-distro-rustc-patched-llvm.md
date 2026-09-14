# 0007: Rust for the card uses the distro rustc with the patched LLVM, not a rustc fork

Status: accepted, 2026-09-13.

## Context

- ADR 0002 makes one patched LLVM the definition of the knc64-x87 ABI for
  both clang and rustc.
- rustc's model of x86-64 hard-codes "the hard-float ABI includes SSE2".
  `Target::abi_required_features()` (`compiler/rustc_target/src/target_features.rs`,
  rustc 1.98.1) requires `x87` and `sse2` for the default ABI and requires
  `soft-float` for the only alternative, `x86-softfloat`. A target spec
  whose feature string contains `-sse2` is rejected while loading:
  "target feature `sse2` is required by the ABI but gets disabled in
  target spec".
- rustc does not itself lower scalar float arguments. `f32`/`f64` become
  LLVM `float`/`double` parameters (`PassMode::Direct`,
  `rustc_target/src/callconv/x86_64.rs`) and the backend chooses register
  or stack. With the patched LLVM and SSE off, Rust gets the same
  convention as C without any rustc change; only the consistency check is
  in the way.
- Measured with a `no_core` probe through rustc 1.98.1 loading the patched
  `libLLVM.so.22.1`: `extern "C" fn rmul(a: f64, b: f64) -> f64` compiles
  to `fldl 8(%rsp); fmull 16(%rsp); ret`, a 64-bit select becomes a
  branch, and `cfg(target_feature)` lists only `x87`, `fxsr`, `crt-static`.

## Decision

- The card Rust compiler is the distro `rust` package (1.98.1 today) with
  the patched `libLLVM.so.22.1` substituted through `LD_LIBRARY_PATH`
  (dylib variant of `toolchain/llvm/build.sh`, linked with a static
  libstdc++ so its symbol versions match Arch's rustc). `RUSTC_BOOTSTRAP=1`
  unlocks `-Zbuild-std` and JSON targets on the stable compiler.
- The target spec disables the SSE tree with the single entry `-sse`.
  rustc expands it by reverse implication (`!sse` implies `!sse2` ...
  `!avx512*`) in both `cfg(target_feature)` and the backend feature list.
  The consistency check inspects only the names written in the spec, so
  `-sse` passes where `-sse2` is refused. rustc then prints one
  future-incompatibility warning per crate ("target feature `sse2` must be
  enabled to ensure that the ABI of the current target can be implemented
  correctly", rust-lang/rust#116344). The warning is expected output.
- `panic-strategy` is `abort`. libunwind is still built for the card
  (`toolchain/libunwind/`) because `std` on static musl links it for
  backtraces.
- Verdict rule: linked executables are audited without exceptions; the
  `std` staticlib is audited with `--ignore XSAVE` for the one `xgetbv`
  in `std_detect` that sits behind a CPUID.OSXSAVE check and is dropped
  from any program not calling `is_x86_feature_detected!`.

## Consequences

- No nightly, no rustup, no compiler build. The dylib variant of LLVM
  (about 40 minutes) is the only cost beyond the C toolchain.
- The setup is coupled to the distro rustc: the dylib must be the LLVM
  major.minor that rustc links (22.1), and every rustc upgrade needs
  `toolchain/rust/gen-target.sh` rerun. Results documents record the
  versions used.
- The warning will become a hard error in some later rustc. The escape
  hatch is documented and mechanical: build rustc from source against the
  patched LLVM with a small patch adding an `x86-x87` `rustc-abi` variant
  (required `x87`, incompatible `soft-float` and `sse`) plus its spec
  parsing, or pin the rustc version with rustup. Nothing else in the
  project depends on which of the two is chosen.
- Rust code for the card must not call `core::arch` SSE intrinsics.
  `cfg(target_feature = "sse2")` is false, so `core` and `std` take their
  generic paths. Spin loops are safe: LLVM patch 0008 lowers the `pause`
  intrinsic to a one-byte `nop` when SSE2 is off.

## Alternatives rejected

- `rustc-abi = "x86-softfloat"` with `+soft-float`: `f64` would travel in
  general registers, disagreeing with musl and clang at every FFI boundary
  (ADR 0002).
- rustc from source with an x87 ABI variant now: correct, but every
  contributor and every rustc upgrade would pay for a compiler build.
  Kept as the escape hatch above.
- Patching `core::hint::spin_loop` in a copied library tree to remove
  `pause`: fixed in the shared LLVM instead (patch 0008), which also covers
  C's `_mm_pause`.
