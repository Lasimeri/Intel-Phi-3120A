# check-docs.sh

`make docs-check`, the first step of `make check`. The same script in every
repository of the family (Intel-Phi-3120A, Intel-Phi-AVX512, Intel-Phi-Jev,
Mechanical-Jev), line for line apart from `code_dirs`, the places that hold code; here
`host/crates`, `card`, `toolchain`, `tools` and `scripts`. Three rules:

1. **Sibling documentation.** Every `*.rs`, `*.c`, `*.h`, `*.S`, `*.sh`,
   `*.json`, `*.config` under `code_dirs` has a `*.md` with the same stem
   in the same directory. Build directories (`target/`, `build/`),
   `vendor/` and `tokenizers/` are skipped. A `code_dirs` entry that does
   not exist is itself an error (Intel-Phi-AVX512 kept a `toolchain` entry
   after the split, and it was skipped silently for two days).
2. **No em or en dashes** (U+2014, U+2013) in any tracked file. Uses
   `grep -P` with Unicode escapes, so it needs GNU grep with PCRE, which
   Arch's `grep` package provides.
3. **Relative links resolve.** Every Markdown link target that is a
   relative path (with or without a `#fragment`) names a file that exists
   relative to the linking file's directory; URLs (anything with a scheme)
   are skipped. A moved or renamed file fails the check until every link
   follows it. A path in backticks is not a link and is not checked: name
   a sibling repository's file as a link to it on GitHub.

It stays a shell script on purpose: it is repository hygiene run by
`make`, identical across repositories written in different mixes of
languages.
