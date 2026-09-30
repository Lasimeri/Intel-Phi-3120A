# wire.S: frames in and out

## Reading

`fill` reads at most 64 KiB from the device into `rx_buf`, first moving the
bytes still buffered (part of one frame) to the front, as phi-rpc's
`Decoder` drains each frame it takes. The buffer therefore holds at most
one frame and one read (`RXCAP` = 4 + `MAX_FRAME` + 64 KiB) and only its
first pages stay resident. A read of 0 ends the agent ("device closed"),
as does a failed read other than `EINTR`.

`/dev/phirpc` returns short reads (at most what the card-side ring holds,
bounded by its bounce buffer) and short writes (kernel patch 0022,
`knc_rpc_read`, `knc_rpc_write`), and `poll(2)` reports `POLLIN` when the
host has published bytes. Frames therefore arrive in pieces and are cut
here, never assumed whole per read.

`next_frame` cuts one frame, with phi-rpc's rules (`Decoder::next_frame`,
`Msg::decode_body`):

- a length of 0 or over `MAX_FRAME` (1 MiB) loses everything buffered: the
  frame boundary is gone and only new input can restore it;
- an unknown tag, or a body shorter than its fields or with a string that
  is not UTF-8, loses that frame only;
- bytes after the last field are allowed.

`validate` walks every message's layout, including the host-bound ones
(`StatReply`, `TrafficReply`, ...), because the Rust agent decoded those
too: a well-formed `StatReply` sent to the agent is answered with
"unexpected StatReply outside a session", a malformed one is dropped.
Each rejected frame is logged as the Rust agent logged it: "phi-agent: bad
frame from the host: " then "frame length N out of range", "unknown
message tag 0xNN" or "malformed frame body".

The frame `next_frame` returns stays in `rx_buf` until the next `fill`;
every caller consumes it (or copies it, as `Exec` and the file paths are
copied) before reading again.

## Writing

One frame at a time is built in `tx_buf` through the write cursor
(`f_begin(tag)`, the `w_*` routines of `text.S`, `f_send`) and written with
`write_all`, which loops over short writes. A write that fails ends the
agent, as it did the Rust agent (`link.send` errors propagated to `main`).
Since the agent is single-threaded, frames are whole by construction.

`tx_buf` is `2 * MAX_FRAME`: the largest frame the agent builds is a
`StatReply`, which `stat.S` keeps under `MAX_FRAME`.

## The relay cannot deadlock the agent

A write to the device blocks when the card-to-host ring is full, and
while it blocks the agent reads nothing. That is safe because the host
relay (`host/crates/phictl/src/serve.rs`, `run`) drains the card's ring on
every pass independently of what it has queued for the card: host-to-card
bytes wait in its `to_card` buffer, not in a ring the agent must empty
first.
