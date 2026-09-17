# phi-agent

The card end of `phictl exec`, `put`, `get` and `status`. It opens
`/dev/phirpc` (kernel patch 0022, the ring's kind-3 channel as a byte
stream) and serves rpc frames (`host/crates/phi-rpc`) one session at a time:

- `Exec`: spawn the program with a cleared environment plus the one sent,
  pipes on all three standard streams; two threads copy stdout and stderr
  into `Stdout`/`Stderr` frames (written whole under a lock so frames never
  interleave); the main thread feeds `Stdin` frames to the child until
  `StdinEof` or the child ends, then sends `Exit` (exit status, or 128 plus
  the signal). The pumps get 300 ms after the exit to drain the pipes and
  are then abandoned: a process the command left in the background (a
  daemon) may keep the pipes open, and its later output is discarded rather
  than holding the session open.
- `PutOpen`/`PutData`/`PutClose`: write the file with the requested mode,
  `fsync`, answer `Exit(0)` or `Error`.
- `Get`: stream the file as `GetData` frames, then `GetEnd { size }`.
- `Ping`: `Pong { version }`.

Errors in a request answer with `Error` and end the session; a corrupt
frame resets the decoder. The agent runs as root on a RAM-only system that
the host resets at will; the security boundary is the daemon's socket on
the host (`host/crates/phictl/src/serve.md`), not this program. Started by
`/init` with its messages on the console.

## Samples for phitop (2026-09-17)

`Stat` is answered from every state: outside a session, inside a running
command, and while a file comes in. The command loop therefore keeps
reading frames after the host closed the command's input (it used to
stop and block in `wait`), polling the child every 20 ms; input frames
that arrive after that are dropped. Stray `Stdin`, `StdinEof`, `PutData`
and `PutClose` frames outside a session are ignored rather than answered
with `Error`, because the daemon closes a departed client's input after
the fact and such a frame may land after the session already ended. The
sample itself is `stat.rs` (`stat.md`).
