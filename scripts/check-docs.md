# check-docs.sh

Enforces the three mechanical rules from `CONTRIBUTING.md`:

1. **Sibling documentation.** Every `*.rs`, `*.c`, `*.h`, `*.S`, `*.sh`,
   `*.json`, `*.config` under `host/crates`, `card`, `toolchain`, `tools`,
   and `scripts` must have a `*.md` with the same stem in the same directory.
   Build directories (`target/`, `build/`) and `vendor/` are skipped.
2. **No em or en dashes** (U+2014, U+2013) in any tracked file. Uses
   `grep -P` with Unicode escapes, so it needs GNU grep with PCRE, which
   Arch's `grep` package provides.
3. **Relative links resolve.** Every Markdown link target that is a
   relative path (with or without a `#fragment`) must name a file that
   exists relative to the linking file's directory; URLs (anything with a
   scheme) are skipped. A moved or renamed file therefore fails the check
   until every reference follows it.

Run by `make docs-check` and as the first step of `make check`.
