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

2026-09-14: executables. The built-in musl spec lets rustc supply its own
start files (`crt-objects-fallback = "musl"`, `pre/post-link-objects-fallback`)
and link static-PIE, which only works with rustc's bundled targets. For the
card, `knc-cc` and the musl sysroot provide `crt1.o`, `crti.o`, `crtn.o` and
`libc.a`, so the generator now sets `crt-objects-fallback = "false"`, drops
the fallback object lists, and links plain static, non-PIE
(`position-independent-executables = false`,
`static-position-independent-executables = false`,
`relocation-model = "static"`). Static libraries (the P2 check) were never
affected; `card/agent` is the first executable.
