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
- One client at a time. If a client disconnects during a command, the daemon
  sends `StdinEof` to the card and keeps draining the card's frames until
  that session's `Exit`, so the next client starts on a clean channel.

The loop runs at 1 kHz (a 1 ms read timeout on the client socket); the
card's `/dev/phirpc` polls at the same rate.
