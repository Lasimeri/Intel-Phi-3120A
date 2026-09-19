# 0006: musl is the card libc

Status: accepted, 2026-09-13.

## Context

The card libc must be built with the knc64-x87 compiler and contain no SSE,
CMOV, fence, or prefetch instructions. glibc's x86-64 port is dense with
hand-written SSE/AVX assembly selected by IFUNC at runtime and assumes the
SysV float ABI in its own assembly. musl's x86_64 port has a small set of
`.s` files, each with a C fallback.

## Decision

musl, static linking by default, dynamic linking supported once the loader
is validated. Rust's `std` for musl targets works with `crt-static`.

## Consequences

- Programs that assume glibc-isms need patches or are skipped.
- `getaddrinfo`, locales, and `dlopen` behave the musl way.
- dropbear, busybox, zlib, ncurses and CPython build against musl upstream
  with no source patches (verified 2026-09-13 to 2026-09-14). QuickJS and
  tcc are expected to and have not been built.

## Alternatives rejected

- glibc: the IFUNC/asm surface is large and every path must be audited.
- uClibc-ng: smaller community, no advantage over musl here.
