# Project instructions for AI assistants

Read `CONTRIBUTING.md` first; it is the authority. Summary of the non-obvious
rules:

- Assembly for the programs on the cards' path (`host/asm/phictl`, the
  daemon and client; `card/agent`, the card's agent): no libc, binutils
  only, verified against a reference before replacing anything. Rust for
  `phi-rpc` (the wire's reference and the agent's test harness),
  `phi-isa-audit`, `knc-mvex`, `phitop`. C only for the card kernel,
  third-party C patches, and `tcc`-compiled helpers. No Python for
  tooling, ever.
- Every code file gets a sibling `.md` with the same stem. Write it in the same
  change as the code.
- No em dash characters in any file.
- Every hardware claim cites a source: Intel document + section, source tree +
  file + function, or a measurement with the command.
- `vendor/` is reference-only and git-ignored. Do not import code from it.
- Hardware tests must not run by default; they are gated on `PHI_BDF`.
- This is the family's base (`CONTRIBUTING.md`, "The family"):
  Intel-Phi-AVX512 builds on it, Intel-Phi-Jev and Mechanical-Jev on
  that. What they consume here (the `phi` verbs, `phi-env.sh`, `phictl`,
  the windows, the control sockets, the SSH forward, patch 0030) keeps
  working: add, do not rename. `knc-mvex`'s library is carried by
  Intel-Phi-AVX512 byte for byte; change the encoder in both.
- Run `make check` before committing.
