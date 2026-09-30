# defs.inc: what every source includes

- **The GNU-stack note**, so `ld` marks the stack non-executable without
  a warning.
- **System call numbers** from `arch/x86/entry/syscalls/syscall_64.tbl`.
  The card runs an x86-64 kernel (v7.2.3 plus this repository's patches),
  so the numbers are the host's, and the same binary runs on both, which
  is how the host tests run it.
- **Flags** from the uapi headers named beside them: `open`, `fcntl`,
  `poll`, signals, `wait4`, clocks, the errno values the code tests,
  `MADV_DONTNEED`, `AT_PAGESZ`.
- **The wire**: `MAX_FRAME`, `MAX_STRING`, `MAX_LIST` and the message tags,
  which must match `host/crates/phi-rpc/src/lib.rs` (its `tags_are_stable`
  test pins the tags; `host/crates/phi-rpc/tests/agent.rs` exercises every
  one the agent handles).
- **The agent's limits**, the Rust agent's: `CHUNK` (32 KiB per output
  frame), `CHILD_POLL_MS` (20), `OUTPUT_GRACE_MS` (300), `INPUT_BACKLOG`
  (1 MiB), `READ_CHUNK` (64 KiB per device read).
- **Buffer sizes**, all `.bss`: `RXCAP` (one frame and one read), `TXCAP`
  (two frames' worth; the largest built is a `StatReply` under
  `MAX_FRAME`), `INCAP` (4 MiB: the input backlog plus what the decoder can
  still hand over), `ARENACAP` (8 MiB: the largest `Exec` a 1 MiB frame can
  hold, as pointer arrays, environment records and C strings), `LOGCAP`,
  `PATHCAP`. They are paged in as touched; `exec.S` hands the input queue
  back after each command.
- **`SYS nr`**: the system call with its arguments already in place;
  returns `-errno` on failure and clobbers `rcx` and `r11`, as the
  `syscall` instruction does.
