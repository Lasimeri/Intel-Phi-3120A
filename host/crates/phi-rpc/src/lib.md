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

## Host-only messages (2026-09-16)

`Sensors` (tag 16) and `SensorsReply` (tag 17) travel only between a
client and the host daemon: the daemon answers a `Sensors` request with
the card's SBOX readings as text and never relays it to the card, so the
agent does not see these tags. `phictl sensors` uses them.

## Telemetry messages (2026-09-17)

`Stat` (tag 18) asks the card for one sample; `StatReply` (tag 19)
carries it as the `Stat` structure: per-CPU busy and idle ticks with the
core id, memory, load averages, the hwmon temperatures, voltage and
clock, per-device byte counters and the process table (fields documented
on the structure; sizes are `u16` counts, integers little-endian, the
process state a byte, temperatures `i16` with -1 for absent). Unlike a
session request, `Stat` may be sent while a session is open: the agent
answers it from every state, and the daemon routes replies to requesters
in order (`phictl/src/serve.md`). `Traffic` (tag 20) and `TrafficReply`
(tag 21) are host-only like `Sensors`: the daemon's PCIe counters, six
`u64`: bytes by DMA into the card and out of it, bytes by the aperture
in each direction, and DMA copies in each direction (2026-09-22; a
daemon and a client are always the same build, so the two new fields
are plain trailing fields, not optional ones). `phitop` is the consumer
of both; `phictl traffic` prints the counters.
