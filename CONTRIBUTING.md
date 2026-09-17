# Contributing and conventions

These rules exist so that someone with the same card and a fresh Arch Linux
install can reproduce every result here without asking anyone.

## Languages

- **Rust** for everything that can be Rust: all host-side software, all tooling
  that runs on the host, and card userland where the toolchain permits.
- **C** only where Rust is not an option: the card kernel and its modules,
  patches to third-party C projects (tcc, QuickJS, CPython, musl), and small
  layout-check helpers that must see the C headers as C sees them. Those
  helpers are compiled and run with `tcc`.
- **Shell** (`sh`, POSIX where practical, `bash` when arrays are needed) for
  setup and glue scripts that orchestrate system tools.
- **Never Python** for project tooling. CPython is a *product* this project
  builds for the card; it is not used to build anything.

## Documentation

- Every code file (`.rs`, `.c`, `.h`, `.S`, `.sh`, `.json` target specs,
  `.config` fragments) has a sibling `.md` with the same stem in the same
  directory. The sibling explains: purpose, the hardware or document facts the
  code depends on (with the source named), invariants, and how to test it.
  Obvious code is not re-narrated; the sibling carries what the code cannot.
- Every public Rust item has a doc comment. Every C function has a comment
  block. Comments explain intent and hardware contract, not syntax.
- Every hardware claim names its source. Acceptable sources are: an Intel
  document number and section, a file and function in a named source tree,
  or a measurement made on this machine with the command shown.
- No em dash characters anywhere in the repository. Use commas, colons,
  parentheses, or `--`.
- Relative links between Markdown files must resolve; `scripts/check-docs.sh`
  enforces the sibling rule, the em dash rule and the link rule (`make
  docs-check`, the first step of `make check`).
  `make check` runs it together with `cargo fmt --check`, `cargo clippy`,
  and `cargo test`.

## Reproducibility

- Anything that touches the system (packages, udev, modules, limits) lives in
  `scripts/` and is idempotent.
- Anything downloaded is fetched by `scripts/fetch-vendor.sh` into `vendor/`
  with a pinned URL and a SHA-256 that is checked. `vendor/` is git-ignored.
- Hardware-dependent tests are behind the `hardware` Cargo feature or an
  explicit `PHI_BDF` environment variable, so `cargo test` passes on a machine
  without the card.
- Results measured on hardware are recorded in `docs/` with the date, the host
  kernel version, and the exact command.

## Git

- Small commits with a scope prefix: `docs:`, `host:`, `card:`, `toolchain:`,
  `scripts:`, `tools:`.
- Never commit anything from `vendor/`, and never commit Intel binaries, flash
  images, or MPSS packages.
