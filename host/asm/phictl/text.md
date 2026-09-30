# text.S: bytes, text and numbers

The routines every other file uses. Nothing here allocates: buffers are
`.bss`.

**The write cursor.** `wp` points where the next byte goes; the `w_*`
routines append there: bytes, little-endian integers, NUL-terminated
strings, decimal (`w_dec`, `w_decw` right-aligned, `w_sdec` signed),
hex (`w_hex` as Rust's `{:#x}`, `w_hexw` as `{:#010x}` for width 8,
`w_hex2` as `{:02x}`), a scaled fraction (`w_fixed`, for the `{:.3}`
seconds the Rust tool printed from integer milliseconds), padding and
left-aligned fields, an OS error as Rust displays one (`w_oserr`: the
message and " (os error N)"), and wire strings (`w_str_begin`,
`w_str_end` clipping to 65535 bytes on a UTF-8 boundary as phi-rpc's
`clip`).

**Output.** Standard output is buffered in `out_buf` between `o_begin`
and `o_end`, flushed at half full, by `o_flush`, and before exit;
standard error lines go out at once (`l_begin`, `l_end`, with
`l_tag` for the "[phictl] " the daemon prefixes and `l_elapsed` for its
"+   1.234s "). `fail` and `fail_os` print "phictl: ..." and exit 1.
`p_begin` and `p_end` build a NUL-terminated path in `path_buf`. All of
these save and restore the cursor, so a line can be logged in the
middle of a frame.

**Strings and scanning.** `cstr_len`, `eq_cstr`, `eq_cc`, `starts_cstr`,
`trim`, `next_token` (ASCII whitespace, as Rust's `split_whitespace`),
`next_line`, `find_byte`, `base_name`, `copy_cstr`.

**Numbers.** `parse_u64` is Rust's `u64::from_str` (an optional `+`,
digits only, no overflow); `parse_hex`, `parse_num` (decimal or `0x`,
the tool's `parse_u64`), `parse_u32`, `parse_u16`, `parse_octal`,
`parse_size` (K, M, G; a positive multiple of 2 MiB, the tool's
`parse_size`), `parse_ipv4`.

**Time and sleep.** `now_ms` and `now_us` on `CLOCK_MONOTONIC`,
`now_ns_realtime` for the host epoch the region header carries,
`sleep_us`, `sleep_ms`, `poll_ms` (errors count as nothing ready).

**Files.** `read_file` (whole, into a buffer), `read_text` (empty on any
failure, as `read_to_string(..).unwrap_or_default()`), `read_link`,
`map_file_whole` (a private copy-on-write map of a file, so the loader
can patch the kernel header without touching the file), `getenv` over
the process's environment.

**errno_text**: glibc's wording for the errno values the tool meets
(the Rust tool ran over glibc); "Unknown error" otherwise.

Tested through the tool: `phictl cards` (formatting and the config
parser) is compared with the Rust tool's output byte for byte in
`docs/results/2026-09-29-phictl-assembly.md`; every number parser is
exercised by the option handling.
