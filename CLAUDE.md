# Project instructions for AI assistants

Read `CONTRIBUTING.md` first; it is the authority. Summary of the non-obvious
rules:

- Rust first. C only for the card kernel, kernel modules, third-party C
  patches, and `tcc`-compiled layout helpers. No Python for tooling, ever.
- Every code file gets a sibling `.md` with the same stem. Write it in the same
  change as the code.
- No em dash characters in any file.
- Every hardware claim cites a source: Intel document + section, source tree +
  file + function, or a measurement with the command.
- `vendor/` is reference-only and git-ignored. Do not import code from it.
- Hardware tests must not run by default; they are gated on `PHI_BDF`.
- Run `make check` before committing.
