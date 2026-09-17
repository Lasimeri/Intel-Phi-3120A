# ADR 0009: telemetry rides the rpc channel as a sideband, and the daemon serves several clients

Date: 2026-09-17. Status: accepted.

## Context

The user asked for a remote resource viewer for the card: per-thread
load, temperatures, memory, PCIe bandwidth and the process list, live.
The card side already runs an agent over the rpc channel (ADR 0008),
but that channel carries one session at a time, and the daemon served
one client at a time. A viewer built on it would freeze whenever a
benchmark ran through `phictl exec`, which is exactly when it is wanted.

Options considered:

1. A card-side sampler writing into shared memory the host reads
   directly (the host memory window of patch 0026, or a ring channel
   read through the aperture). No daemon involvement, but the swap
   device owns the whole host memory window, so a slice would have to be
   carved out (kernel and protocol change), and userland on the card has
   no clean way at a ring channel without another device.
2. Full session multiplexing on the rpc channel (session ids in every
   frame). Correct and general, but it changes every message and both
   ends for one consumer.
3. A sideband: one request (`Stat`) that the agent answers from any
   state, and a daemon that admits several clients, hands the card's
   session to one of them at a time, and routes `Stat` replies to their
   requesters in order.

## Decision

Option 3. `Stat` and `StatReply` join `phi-rpc`; the agent answers
`Stat` outside a session, inside a running command (its input loop no
longer stops at end of input) and during a put. The daemon keeps up to
16 connections, tracks the session from the holder's own frames, and
interleaves `Stat` frames with it. PCIe traffic is counted where the
host moves bytes (`phi_vfio::traffic`: aperture copies and DMA
completions) and reported by a host-only `Traffic` message. `phitop`
(host, Rust, no TUI crate) consumes both.

## Consequences

- One frame per second of a few kilobytes on the rpc channel; the
  agent's sampling costs one read of /proc/stat and one stat and status
  read per user process; the 1500 kernel threads are re-read every
  fourth sample (reading them every second cost a sixth of a hardware
  thread) and sent only when their tick counts moved.
- Frames from the agent are whole (its writer is a mutex), so a
  `StatReply` never splits a session's output.
- A `StatReply` from an agent older than the daemon cannot happen (both
  are built from one source), but an old agent would answer `Stat` with
  `Error`, which the daemon reads as a session end; the initramfs and the
  host tools must be rebuilt together, as before.
- Replies are routed in request order, which is right because the
  channel is in order and the agent answers each `Stat` before reading
  the next frame.

## Alternatives rejected

Options 1 and 2 above. Also rejected: reading the card's counters from
the host through the aperture without an agent (would need every /proc
value laid out in memory by a card program anyway), and a second rpc
device on the card (a kernel change for one consumer).
