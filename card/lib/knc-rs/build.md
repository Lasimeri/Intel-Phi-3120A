# build.sh and build.rs (knc-rs)

Builds the `knc` crate and `knc-demo` for the card, audits the binary, and
copies it where `phi put` can reach it.

```sh
card/lib/knc/build.sh          # first: this links libknc.a out of the sysroot
card/lib/knc-rs/build.sh
phi put ~/.cache/intel-phi-3120a-build/userland/knc-rs/knc-demo /tmp/knc-demo
phi run sh -c 'chmod +x /tmp/knc-demo && /tmp/knc-demo 228 64 256'
```

Mirrors `card/agent/build.sh`, for the same reasons (ADR 0007):

- the patched `libLLVM.so.22.1` through `LD_LIBRARY_PATH`, because the
  distro rustc must load the LLVM that knows this card's ABI;
- `RUSTC_BOOTSTRAP=1`, because a JSON target and `-Zbuild-std` are unstable
  and there is no prebuilt `std` for `x86_64-knc-linux-musl`;
- `cargo clean` every time, because cargo does not fingerprint the dylib and
  would otherwise reuse objects built against the wrong one.

One warning per crate is expected output: "target feature `sse2` must be
enabled to ensure that the ABI of the current target can be implemented
correctly". ADR 0007 explains why it is there and what the escape hatch is.

The audit must come back clean, and it reports about 19838 KNC vector
instructions, which is `libknc` having been linked in. A count near zero
would mean the library was dropped.

## build.rs

Points the linker at `libknc.a` and nothing else.

The library is generated assembly, about 20500 lines emitted by
`knc-mvex-gen codec`, and building it belongs to `card/lib/knc/build.sh`,
which also audits it and installs it into the sysroot. Doing that work from
`build.rs` instead would mean cargo owning a build step that has to run
before anything else on the card can link, and would hide the audit.

So this script only resolves the sysroot (`PHI_SYSROOT`, with the default
`toolchain/env.sh` sets as the fallback), fails with a pointer to the right
script if `libknc.a` is not there yet, and emits the two `cargo:` lines.
