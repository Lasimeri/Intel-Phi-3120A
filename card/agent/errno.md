# errno.S and errno.sh

The Rust agent reported a failed system call as Rust displays an
`io::Error` from an errno: the C library's message, then " (os error N)"
(`std::sys::os::error_string`, which calls `strerror_r`). The C library
was musl, so the messages are musl's, and they differ from glibc's in
places ("No error information" for an unknown code, for one). `errno.S`
carries musl 1.2.5's table so the assembly agent words every error the
same way.

`errno.sh [MUSL_DIR]` writes `errno.S` from the musl source the toolchain
fetches (`toolchain/fetch.sh`, pinned in `toolchain/SHA256SUMS`):

- the texts from `src/errno/__strerror.h`, whose `E(code, "text")` lines
  musl includes to build its own table;
- the numbers from `arch/generic/bits/errno.h` (x86-64 has no errno.h of
  its own), aliases such as `ENOTSUP` for `EOPNOTSUPP` resolved.

The table is musl's shape: one u16 offset per errno into a block of
NUL-terminated strings, a code with no text pointing at entry 0 ("No error
information"), a code past the table reading as entry 0
(`src/errno/strerror.c`, `__strerror_l`). `errno.S` is committed, so a
checkout builds without the toolchain; regenerate and diff it after a musl
update:

```
card/agent/errno.sh > card/agent/errno.S && git diff --stat card/agent/errno.S
```
