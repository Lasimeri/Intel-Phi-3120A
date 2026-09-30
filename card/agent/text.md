# text.S: bytes, text and numbers

The routines every other file uses. None allocates.

**The write cursor.** `wp` points where the next byte goes; `w_byte`,
`w_u16`, `w_u32`, `w_u64` (little-endian, as the wire and x86 are),
`w_bytes`, `w_cstr`, `w_dec` (unsigned decimal), `w_hex` (`0x` and
lowercase digits, Rust's `{:#x}`), `w_oserr` (Rust's display of an OS error:
musl's message, then " (os error N)") and `w_ioerr` append there. A frame,
a log line and a path are all built this way. `w_str_begin`/`w_str_end`
bracket a wire string (u16 length, bytes) written in pieces; the end clips
it to 65535 bytes on a character boundary, as phi-rpc's `clip` does, so a
long path in an error still arrives as valid UTF-8.

**Logging.** `l_begin` (with the "phi-agent: " prefix) or `l_begin_bare`,
then `l_end`, which writes the line to standard error with its newline.
The cursor is saved and restored around it, so a line can be logged while
a frame is being built. `p_begin`/`p_end` do the same for a path in
`path_buf`.

**Scanning.** `next_token` (fields split on ASCII whitespace, as Rust's
`split_whitespace` splits /proc text), `next_line` (Rust's `lines`: `\n`
or `\r\n`, no empty last line), `trim`, `eq_cstr`, `starts_cstr`.

**Numbers.** `parse_u64` is Rust's `u64::from_str`: an optional `+`, at
least one digit, nothing else, no overflow (`mulq` sets the carry when the
product leaves 64 bits). `parse_u32` and `parse_u16` add the range;
`parse_i64` takes `-` too and the asymmetric range. `hundredths` is the
Rust agent's: the whole part and at most two decimals, parsed separately
("1.33" is 133, "0.5" is 50, "1.234" is 123, anything unparsable 0).

**UTF-8.** `utf8_ok` accepts what `str::from_utf8` accepts: no overlong
forms (C0, C1, E0 below A0, F0 below 90), no surrogates (ED above 9F),
nothing past U+10FFFF (F4 above 8F, F5 and up).

**Files.** `read_text` is `fs::read_to_string(path).unwrap_or_default()`:
the whole file (up to the buffer), or nothing when it cannot be opened or
read or is not UTF-8. `open_dir` opens a directory. Both put `root_ptr`
(`--root`) in front of the path. `now_ms` is the monotonic clock in
milliseconds; `poll_ms` counts `EINTR` and failures as nothing ready, as
the Rust agent's `poll` did.

Every one of these is exercised through the agent by
`host/crates/phi-rpc/tests/agent.rs`; the number rules by the fixture test
(a capped load, "x" as a core id, a negative millidegree reading), UTF-8 by
the bad-frame and non-UTF-8 name cases, clipping and `w_oserr` by the error
texts.
