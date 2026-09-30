# client.S: exec, put, get, status, sensors, traffic

Each verb is one connection to the control socket (`--socket`, else
`PHICTL_SOCKET`, else the root path when running as root or when a root
daemon answers there, else the user's path), one request, the replies.

- `exec [--cwd DIR] [--] PROG ARGS`: `Exec` with the card environment
  (`PATH=/opt/phi/bin:/bin:/sbin:/usr/bin:/usr/sbin`, `HOME=/root`,
  `TERM=dumb`); standard input and the daemon's socket in one poll loop
  (the Rust client read standard input on a thread): input goes out as
  `Stdin` frames of up to 16 KiB, end of file as `StdinEof`; `Stdout` and
  `Stderr` frames are written through; `Exit` becomes the exit status;
  `Error` prints "phictl exec: ..." and exits 255.
- `put SRC DST [--mode OCTAL]`: `PutOpen` with the mode (the local file's
  otherwise), `PutData` in 64 KiB frames, `PutClose`; "phictl put: N
  bytes" on standard error.
- `get SRC DST`: `Get`, `GetData` frames into the local file (created
  0644 at the first frame, or empty at `GetEnd`), "phictl get: N bytes".
- `status`: `Ping`, "card agent VERSION reachable through SOCKET".
- `sensors`, `traffic`: the daemon's replies printed as the Rust client
  printed them.

An `Error` reply prints its text and exits 1; a frame that phi-rpc would
reject, or a closed connection, exits 1 with the reason.

Verified against the Rust daemon on card 0 and the assembly daemon on
card 1: exec with input, both streams and the status; a signal; `--cwd`;
put with a mode; get byte-identical; sensors and traffic in the Rust
format (`docs/results/2026-09-29-phictl-assembly.md`).
