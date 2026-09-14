# card/agent manifest

A separate Cargo package, not a member of the host workspace: it is built
for the card target only (`.cargo/config.toml`, `build.sh`). Depends on the
shared `phi-rpc` framing crate by path and on `libc` for `poll(2)`.
`panic = "abort"` matches the target spec.
