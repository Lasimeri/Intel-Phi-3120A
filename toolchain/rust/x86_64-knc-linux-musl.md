# x86_64-knc-linux-musl.json

Custom Rust target for the card. Derived from the built-in
`x86_64-unknown-linux-musl` (`compiler/rustc_target/src/spec/targets/`)
with these changes:

| Field | Value | Why |
| --- | --- | --- |
| `features` | every SIMD family, CMOV, MOVBE, POPCNT, LZCNT, BMI, XSAVE, CX16, PRFCHW, CLFLUSHOPT, RDRND, RDSEED, ADX, FSGSBASE, PCLMUL, AES, SHA disabled; `+x87` | The deletion list (`docs/research/isa-deletions.md`). Rust knows these names (`rustc --print target-features`). |
| `cpu` | `x86-64` | The baseline; features above override its implications. Requires the patched LLVM so that `-cmov` is honored in 64-bit mode and `f64` returns do not error. |
| `max-atomic-width` | 64 | No `CMPXCHG16B`. (The musl target already says 64.) |
| `panic-strategy` | `abort` | Unwinding needs `libunwind` built for the card; deferred. |
| `vendor` | `knc` | Cosmetic; lets `cfg(target_vendor = "knc")` gate card-specific code. |
| `crt-static-default` | true | Static binaries until the musl dynamic loader is validated on the card. |

## Regenerating

Field names change between Rust releases. When they do, regenerate the
base with a nightly toolchain and re-apply the table above:

```
rustc +nightly -Zunstable-options --print target-spec-json \
    --target x86_64-unknown-linux-musl > base.json
```

## Building

```
cargo +nightly build -Zbuild-std=std,panic_abort \
    --target toolchain/rust/x86_64-knc-linux-musl.json
```

with `CC_x86_64_knc_linux_musl=knc-cc` and the linker set to `knc-cc` in
`.cargo/config.toml` so that `compiler-builtins` and `libc` link against
the musl sysroot. `rustup component add rust-src` is required.
