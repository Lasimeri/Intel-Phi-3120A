# serve.rs: the control socket of `phictl boot --serve`

The process that boots the card owns the VFIO group, so it is the only
process that can reach the ring region. `--serve [PATH]` makes it relay rpc
frames (`phi-rpc`) between one local client at a time and the card's agent
over the ring's kind-3 channel, so that `phictl exec`, `put`, `get` and
`status` work from an unprivileged shell without SSH or any network.

Security model (`docs/decisions/0008-direct-access-tool.md`):

- The socket's directory is root-owned with mode 0711 (traversable, not
  listable); the socket file is owned by `--owner` (default `SUDO_UID`,
  else root) with mode 0600.
- Every accepted connection is checked with `SO_PEERCRED`: only the owner
  uid and root are served; others are closed at once.
- The daemon interprets frames only to find their boundaries and to know
  when a session ends; it never executes anything on the host and never
  writes card data anywhere but the client socket.
- One client at a time holds the card (2026-09-17: several may be
  connected; see below). If the holder disconnects during a command the
  daemon sends `StdinEof` (or `PutClose` during a put) and keeps draining
  the card's frames until that session ends, so the next client starts on
  a clean channel.

The loop runs at 1 kHz (a 1 ms read timeout on the client socket); the
card's `/dev/phirpc` polls at the same rate.

## Several clients, one session, samples in between (2026-09-17)

Up to 16 connections are accepted. The card's agent still runs one
session at a time, so the daemon hands the card to one client (the
*holder*: a command, a transfer, a ping) and lets the others wait: a
waiting client's first session frame is decoded and held, its later
bytes stay in its socket until its turn, and clients get the card in
connection order.

Three requests never wait:

- `Sensors` and `Traffic` are answered by the daemon itself (SBOX
  registers, `phi_vfio::traffic` counters).
- `Stat` is forwarded to the card at once, even mid-session, because the
  agent answers it from every state (`card/agent/src/main.md`); the
  daemon queues the requester's id and routes each `StatReply` to the
  oldest waiting requester. This is what keeps `phitop` live while a
  benchmark runs through `phictl exec`.

Session state is tracked from the holder's own frames (`Exec` until
`Exit`, `PutOpen` until the card's `Exit`, `Get` and `Ping` until
`GetEnd` and `Pong`), so a holder that leaves is closed out correctly:
`StdinEof` only if it had not sent one, `PutClose` only during an open
put, nothing for a get or a ping.
