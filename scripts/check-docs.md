# check-docs.sh

Enforces the two mechanical rules from `CONTRIBUTING.md`:

1. **Sibling documentation.** Every `*.rs`, `*.c`, `*.h`, `*.S`, `*.sh`,
   `*.json`, `*.config` under `host/crates`, `card`, `toolchain`, `tools`,
   and `scripts` must have a `*.md` with the same stem in the same directory.
   Build directories (`target/`, `build/`) and `vendor/` are skipped.
2. **No em or en dashes** (U+2014, U+2013) in any tracked file. Uses
   `grep -P` with Unicode escapes, so it needs GNU grep with PCRE, which
   Arch's `grep` package provides.

Run by `make docs-check` and as the first step of `make check`.
