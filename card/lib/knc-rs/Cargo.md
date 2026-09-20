# Cargo.toml (knc)

One crate, two artifacts: the `knc` library (the bindings, `src/lib.md`)
and the `knc-demo` binary (`src/main.md`).

`panic = "abort"` in both profiles, as ADR 0007 requires for the card.

No dependencies. The crate is a set of `extern "C"` declarations and two
aligned newtypes; everything it calls is in `libknc.a`, which `build.rs`
points the linker at.

`.cargo/config.toml` carries the card target spec, `-Zbuild-std` (no
prebuilt `std` exists for a custom target) and `knc-cc` as the linker, the
same three settings `card/agent` uses.
