# toolchain/rust/gen-target.sh

Produces `x86_64-knc-linux-musl.json` from the installed compiler:

1. `rustc -Zunstable-options --print target-spec-json --target x86_64-unknown-linux-musl`
   (with `RUSTC_BOOTSTRAP=1` on a stable toolchain) emits the built-in
   musl spec in the exact schema this rustc reads. Field types change
   between releases (1.98 turned `target-pointer-width` into a number,
   which broke the hand-written file), so generating is the only way to
   stay correct.
2. `jq` sets `vendor` to `knc`, `features` to the deletion list (with
   `-sse` standing in for the whole SSE tree; `x86_64-knc-linux-musl.md`
   explains why `-sse2` must not appear literally), `panic-strategy` to
   `abort`, and pins `cpu`, `max-atomic-width` and `crt-static-default`.

Re-run after every rustc upgrade; `build-std.sh` fails with "error
loading target specification" when the file is stale. `jq` is in the
`setup-arch.sh` package list.
