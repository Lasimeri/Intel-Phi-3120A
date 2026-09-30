# net.S: `--net NAME`, the TAP bridge

The root path to the card's network (`host/crates/phictl/src/net.rs`):
a TAP device created or attached (`/dev/net/tun`, `TUNSETIFF` with
`IFF_TAP | IFF_NO_PI`, needs root), given the `--net-addr` address and
brought up through iproute2 (`ip addr replace ADDR dev NAME`, `ip link
set NAME up`, run by fork and execve as the Rust bridge ran them), then
bridged to the ring's network channel: one frame from the TAP per pass
into the host-to-card ring whole (dropped when the ring is full, like a
NIC queue), everything the card published cut into records and written
to the TAP. A record that cannot be the card's resynchronises by
dropping the buffer. Counts (frames each way, dropped, bad) are logged
every 30 s when they moved. Mutually exclusive with `--forward`, which
consumes the same ring.

Not exercised on this host since the port: the units run unprivileged
and use `--forward`.
