# forward.S: `--forward HOSTPORT:CARDPORT`

Root-free TCP forwarding to the card (`host/crates/phictl/src/forward.rs`):
phictl is the host end of the ring network with MAC `02:50:48:49:00:01`
and the `--net-addr` address (`10.9.N.1` for card N; the card is `.2`),
listening on `127.0.0.1:HOSTPORT` (a port alone means card port 22).
Each accepted connection (nonblocking) becomes a connection of the stack
in `tcp.S` to `CARDPORT` on the card; bytes are shuttled both ways by
the stack's step. At most 32 connections at once; a 33rd is closed with
a log line. The listener is created once the card has reached init and
the network channel is found. `forward_fds` gives the daemon's idle
wait the listener and the connections' host sockets (`tcp.md`), so a
new connection or bytes from ssh end the wait at once.

`ssh -p 2222+N root@127.0.0.1` with the card key then reaches the card's
dropbear from an unprivileged boot, which is how `phi sh`, `phi put` over
scp and the co-processor's deploy reach it.

Measured on card 1 (`docs/results/2026-09-29-phictl-assembly.md`):
16 MiB up over ssh in 4.4 s; the Rust forwarder measured about
0.6 MB/s on 2026-09-15.
