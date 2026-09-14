# card/agent/.cargo/config.toml

Selects the card target spec, `build-std` (core, alloc, std, panic_abort)
because no prebuilt std exists for a custom target, and `knc-cc` as the
linker. `build.sh` supplies `RUSTC_BOOTSTRAP=1` and the LLVM dylib path.
