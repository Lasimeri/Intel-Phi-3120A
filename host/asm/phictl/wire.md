# wire.S: rpc frames on the host

The framing of `host/crates/phi-rpc/src/lib.rs` (`u32 length | u8 tag |
body`; strings as `u16 length | UTF-8`; lists as `u16 count | items`;
frames at most 1 MiB), as the daemon's relay and the client verbs need
it.

**Decoders.** One per client connection and one for the card, each a
64-byte record over its own buffer (a frame and one read): `dec_room`
moves the buffered part to the front and says where a read may land,
`dec_advance` counts the read in, `dec_next` cuts the next frame. The
rules are phi-rpc's `Decoder::next_frame`: a length of 0 or over the
limit loses everything buffered (the boundary is lost), an unknown tag
or a body shorter than its fields or with a string that is not UTF-8
loses that frame only, bytes after the last field are allowed.
`validate` walks every message's layout, host-bound ones included.
`log_bad_frame` words the rejection as the Rust relay did.

**Building.** `f_begin(tag)` starts a frame in `tx_buf`, the body goes
through the write cursor, `f_finish` fills the length in.

`msg_names` are the names `Msg::name` gave, for logs.

The wire is tested end to end by `host/crates/phi-rpc/tests/agent.rs`
(the card agent against phi-rpc) and, for this side, by running the
assembly client against the Rust daemon and the Rust client against the
assembly daemon (`docs/results/2026-09-29-phictl-assembly.md`).
