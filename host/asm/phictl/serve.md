# serve.S: the control socket

`phictl boot --serve [PATH]` (`host/crates/phictl/src/serve.rs`, ADR
0008 and 0009). The process that booted the card holds the VFIO group
and is the only one that can reach the ring region, so it relays rpc
frames between local clients and the card's agent.

**The socket.** `auto` is `/run/phictl[/N]/control.sock` for root, else
`$XDG_RUNTIME_DIR/phictl[/N]/control.sock`. The directory is created
(every component) and set 0711; a stale socket file is removed; the file
is 0600 and chowned to the owner (`--owner`, else `SUDO_UID`, else the
daemon's uid); the listener is nonblocking. Every accepted connection is
checked with `SO_PEERCRED`: the owner's uid or root, anything else is
closed and logged. At most 16 clients.

**The relay**, the Rust `Relay` state for state:

- A client's frames are read and cut only while it holds the card or has
  nothing pending; otherwise its later frames stay in its socket.
- `Sensors` and `Traffic` are answered here. `Stat` goes to the card at
  once, whatever the session, and the requester's id is queued; each
  `StatReply` from the card goes to the oldest waiting requester.
- Any other frame from the holder is forwarded, the session state
  following it (`Exec` open, `StdinEof` closes its input, `PutOpen`,
  `PutClose`, `Get` and `Ping` as "other"). From a client that does not
  hold the card it is held in that client's pending buffer (two frames'
  worth; more drops the client).
- With no session, the first client with pending frames becomes the
  holder and its frames go out.
- A frame from the card goes to the holder; `Exit`, `Error`, `GetEnd` or
  `Pong` ends the session, and the holder is released even if it left
  (its remaining replies are drained to nobody, so the next client
  starts clean).
- A holder that disconnects mid-session has what it left open closed
  for it (`StdinEof` for a command, `PutClose` for a put), as the Rust
  `abandon` did.

Bytes for the card wait in `to_card` (4 MiB) until the rpc ring takes
them; a client is not read while that buffer is within two frames of
full, which is the backpressure the Rust `Vec` never had. Frames from the
card are popped in 64 KiB pieces into the card decoder.

Nothing the card sends is interpreted beyond frame boundaries; nothing
is executed on the host.

**Movement.** `serve_step` reports movement when a client was accepted,
a client's bytes were read, a waiting client became the holder, bytes
went into the rpc ring or came out of it. `pick_holder` returned the
last client slot's offset instead of 0 when nobody waited until
2026-10-08, which kept the daemon spinning (`console.md`).
`serve_fds` lists the listener and the clients `clients_to_card` would
read next (alive, holding the card or with nothing pending, room in its
decoder and in `to_card`, the rpc channel attached), for the daemon's
idle wait.

**Sensors** are formatted as `sensors_text` did, from the SBOX: the nine
die temperatures (three 10-bit fields per register, 0 shown as n/a) and
the highest maximum, the board temperatures (valid bits), the VDDG
regulator field, the TMU, the VR12 core voltage (250 mV + 5 mV per code
above 1), the core clock from `COREFREQ` and `CURRENT_CLK_RATIO` through
Intel's PLL table (`core_khz`: feedback 8..16 for divider 1, 8..15 for 2
and 4, the ICC divider from SPAD4, 20 when unfused).

Verified: the Rust client and phitop against this relay on card 1, a
sample delivered while a command held the card, a second client queued
and served after it (`docs/results/2026-09-29-phictl-assembly.md`).
