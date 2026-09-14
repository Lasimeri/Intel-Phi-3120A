# card/agent/build.sh

Builds `phi-agent` for the card: Rust, static musl, the card target spec
(`toolchain/rust/x86_64-knc-linux-musl.json`) with `std` built from source,
`knc-cc` as the linker, exactly as `toolchain/rust/build-std.sh` does for
the P2 check. Requirements: the patched LLVM dylib variant
(`PHI_LLVM_VARIANT=dylib toolchain/llvm/build.sh all`) and `rust-src`.

Steps: clean build (cargo does not fingerprint the LLVM dylib), audit with
`phi-isa-audit` (must be clean), copy to
`~/.cache/intel-phi-3120a-build/userland/agent/phi-agent`, from where
`card/initramfs/build.sh` installs it as `/bin/phi-agent`.
