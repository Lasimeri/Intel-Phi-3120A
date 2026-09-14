# ADR 0008: a local, socket-gated tool drives the card instead of SSH

Date: 2026-09-14. Status: accepted.

## Context

Once the card boots and runs a shell over the ring tty, building software
on it needs a way to run commands and move files from the host without a
person at the console, including from an unprivileged automation shell
(the assistant's). SSH over the ring network (P5) exists, but the user asked
for a path that does not use SSH and is as secure as the setup allows.

## Decision

A third ring channel (kind 3, `docs/spec/ring-protocol.md`) carries framed
messages (`host/crates/phi-rpc`): run a program with its three standard
streams and exit status, write a file, read a file, ping. On the card, a
kernel misc device `/dev/phirpc` exposes the rings as a byte stream (kernel
patch 0022) and a Rust agent (`card/agent`) serves the requests. On the
host, the process that booted the card (`phictl boot --serve`) relays
frames between the card and a Unix socket.

The trust model:

- Only the VFIO holder (a root process) can touch the ring region, so only
  the daemon talks to the card.
- The socket is the single entry point: root-owned 0711 directory, socket
  file owned by one uid (0600), `SO_PEERCRED` check on every connection.
  Any other user on the host gets a refused connection, not a prompt.
- The daemon executes nothing on the host and interprets card data only as
  frame boundaries; a hostile card can at most fill the client's terminal.
- The card is a RAM-only system that the host resets at will (VFIO resets
  it on open and close), so the agent running as root on it is not a
  privilege the host lacks anyway.
- No network is involved: the ring is card memory reached through the PCIe
  aperture, invisible to anything but the daemon.

## Consequences

- `phictl exec|put|get|status` work from the owner's shell without `sudo`
  while `sudo phictl boot --serve` runs (typically in tmux).
- One client at a time; concurrency is a later step (session ids are not
  in the frame format).
- Throughput is that of the polled rings (about the same as the network
  channel); fine for builds and file transfer of megabytes, not for bulk
  data.
- SSH remains available for interactive use and `scp`; the tool does not
  replace it, it removes the need for it in automation.
