# Contributing and conventions

These rules exist so that someone with the same card and a fresh Arch Linux
install can reproduce every result here without asking anyone.

## Languages

- **Rust** for everything that can be Rust: all host-side software, all tooling
  that runs on the host, and card userland where the toolchain permits.
- **C** only where Rust is not an option: the card kernel patches (they amend
  C and must read as kernel code), patches to third-party C projects (tcc,
  QuickJS, CPython, musl), the shared ring header, and small helpers under
  `tools/` that must see the C headers as C sees them. Those helpers are
  compiled and run with `tcc`. The project builds no kernel modules: every
  card device is a patch, because the console must exist before a module
  could be loaded.
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
- Anything downloaded has a pinned URL and a checked SHA-256. Build inputs
  (musl, busybox, dropbear, zlib, ncurses, CPython) go through
  `toolchain/fetch.sh` against `toolchain/SHA256SUMS`; LLVM is a depth-1
  clone of a pinned tag. Reference material (MPSS archives, Intel's k1om
  tree, PDFs) goes through `scripts/fetch-vendor.sh` into `vendor/`, which
  is git-ignored and never linked into a build.
- Hardware-dependent tests are behind the `hardware` Cargo feature or an
  explicit `PHI_BDF` environment variable, so `cargo test` passes on a machine
  without the card.
- Results measured on hardware are recorded in `docs/` with the date, the host
  kernel version, and the exact command.

## The family

| repository | what | finds its dependency by |
| --- | --- | --- |
| [Intel-Phi-3120A](https://github.com/Lasimeri/Intel-Phi-3120A) (this one) | the cards' software stack: daemon, kernel, boot, storage, the `phi` CLI, the cross toolchain | (none) |
| [Intel-Phi-AVX512](https://github.com/Lasimeri/Intel-Phi-AVX512) | the cards as an AVX-512 co-processor: phi512, the card worker, the `libggml_phi.so` backend | `PHI_STACK_ROOT`, `phi` on PATH, a checkout next to it, `$HOME` |
| [Intel-Phi-Jev](https://github.com/Lasimeri/Intel-Phi-Jev) | `xks`, a local Jev (System One) whose subject runs on the host and the cards | `PHI_AVX512_ROOT`, a checkout next to it, `$HOME` |
| [Mechanical-Jev](https://github.com/Lasimeri/Mechanical-Jev) | `mjev`, the asking side of Jev, and Jev reverse engineered from its docs | `MJEV_XKS`, `xks` on PATH, a checkout next to it, `$HOME` |

- A dependency is found in that order, as a checkout under its GitHub
  clone's name (`Intel-Phi-AVX512`) or the spaced one (`Intel Phi
  AVX-512`); `phi vpu` finds Intel-Phi-AVX512 that way (`PHI_AVX512_ROOT`,
  next to this checkout, `$HOME`).
- Nothing of a sibling is copied into another, with one exception: the
  `knc-mvex` library (`lib.rs`, `conv.rs`, `transc.rs`), which
  Intel-Phi-AVX512 carries byte for byte and its `make check` compares
  with this one. A change to the encoder is made in both in the same
  session; the generator (`src/main.rs`) is only here.
- The interfaces the others consume from this repository keep working
  across changes:
  - the `phi` command and its verbs (`-c N`, `status`, `run`, `vpu`,
    `swapoff`, `ssh-config`);
  - `scripts/phi-env.sh` (card index to socket, port and window);
  - `toolchain/env.sh` and `toolchain/clang/knc-cc`;
  - `host/target/release/phictl` (or `debug/`);
  - the host-memory windows `/dev/shm/phi-hostmem` and `phi-hostmem-N`;
  - the control sockets `$XDG_RUNTIME_DIR/phictl/control.sock` and
    `phictl/N/control.sock`;
  - the SSH forward `127.0.0.1:2222+N` with `~/.ssh/phi_ed25519`,
    `~/.ssh/known_hosts_phi` and the host key alias `phi`;
  - the card kernel's patch 0030 (the vector unit across a signal
    handler).

  Add, do not rename; when one must change, change its consumers in the
  same session.
- Everything downloaded or built for the card itself (musl, busybox,
  dropbear, CPython, LLVM, the vendor archives) is this repository's,
  pinned and checked here (`toolchain/` and `scripts/fetch-vendor.sh`).

## Git

- Small commits with a scope prefix: `docs:`, `host:`, `card:`, `toolchain:`,
  `scripts:`, `tools:`.
- Never commit anything from `vendor/`, and never commit Intel binaries, flash
  images, or MPSS packages.
