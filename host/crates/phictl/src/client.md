# client.rs: exec, put, get, status

Client side of the control socket (`serve.md`). Each command connects,
sends one request and relays the reply:

- `phictl exec [--cwd DIR] -- PROGRAM ARGS...` sends `Exec` with a fixed
  environment (`PATH`, `HOME=/root`, `TERM=dumb`), pumps local stdin as
  `Stdin` frames (then `StdinEof`), prints `Stdout`/`Stderr` frames as they
  arrive, and exits with the card's status (`255` on an `Error` reply).
- `phictl put SRC DST [--mode OCTAL]` streams the file as `PutData` frames
  after `PutOpen` (mode defaults to the local file's) and expects `Exit(0)`.
- `phictl get SRC DST` writes `GetData` frames to the local file until
  `GetEnd`.
- `phictl status` sends `Ping` and prints the agent version from `Pong`.

`--socket` selects the daemon (default `/run/phictl/control.sock`). The
connection refuses with a hint when no daemon runs or the caller is not the
socket's owner.
