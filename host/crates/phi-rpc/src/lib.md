# phi-rpc: message framing for the rpc channel

Shared by `phictl serve` (host) and the card agent (`card/agent`), which is
built for the card by the card Rust toolchain from the same source, so the
two ends cannot drift apart. The channel itself is ring kind 3
(`docs/spec/ring-protocol.md`); on the card it appears as `/dev/phirpc`
(kernel patch 0022), on the host the daemon reads and writes the rings
directly through `phi-ring`.

Frames: `u32` length, `u8` tag, body; strings are `u16`-length-prefixed
UTF-8; byte payloads run to the end of the frame; frames are at most 1 MiB.
Messages: `Exec` (argv, env, cwd), `Stdin`, `StdinEof`, `Stdout`, `Stderr`,
`Exit`; `PutOpen`, `PutData`, `PutClose`; `Get`, `GetData`, `GetEnd`;
`Error`; `Ping`, `Pong`. One session at a time: the daemon serialises
clients, the agent finishes a command or a transfer before reading the next
request.

`Decoder` reassembles frames from arbitrary chunks (the rings deliver bytes,
not messages); the tests feed the whole message set in 7-byte pieces.
