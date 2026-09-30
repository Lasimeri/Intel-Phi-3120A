# xfer.S: put and get

`PutOpen { path, mode }` opens the file `O_WRONLY | O_CREAT | O_TRUNC`
with `mode`, then `fchmod`s it to `mode`: `open(2)` applies the mode only
to a file it creates and only after the umask, and the host asked for
exactly this mode (the Rust agent's `set_permissions` did the same).
`PutData` bodies are written whole, straight from the receive buffer;
`PutClose` is `fsync`, then `Exit(0)`. `Stat` is answered meanwhile; any
other frame is logged as "dropping <name> during a put" and dropped.

`Get { path }` sends the file in `GetData` frames of at most 32 KiB, read
straight into the frame being built, then `GetEnd { size }` with the bytes
sent. Between chunks, every frame already buffered or arriving right now
is taken: `Stat` is answered, anything else dropped with "dropping <name>
during a get".

Failures are `Error("<path>: <why>")` and end the session, `<why>` being
Rust's wording: musl's message and "(os error N)" for a failed system call,
"failed to write whole buffer" for a write that returned 0, and "file name
contained an unexpected NUL byte" for a path holding a NUL (checked before
any system call, as Rust checks it when making the C string). After a
failed put, the rest of its frames arrive outside a session and are
dropped without a reply (`agent.S`).

Tested in `host/crates/phi-rpc/tests/agent.rs` (`put_and_get_round_trip`,
`file_failures_are_errors`): 100 000 bytes out and back with a `Stat` in
the middle, mode 0640 whatever the umask, a missing directory, a
directory read as a file, a NUL in a path.
