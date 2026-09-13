# init (Rust)

PID 1 on the card, built for the `x86_64-knc-linux-musl` target with
`-Zbuild-std`. Responsibilities, in order:

1. Mount `proc`, `sys`, `dev` (devtmpfs), `tmp`.
2. Set the `CARD_FLAG_INIT_REACHED` bit in the ring region header (the
   host's console loop prints it), using `/dev/mem` or a small ioctl on
   `/dev/ttyPHI0` exposed by `phinet`.
3. Load `phinet`, bring up `phi0` with the address from the kernel
   command line (`phi.ip=`), set the default route to the host.
4. Set the clock from the ring header's `host_epoch_ns` if the kernel did
   not already.
5. Start `dropbear`, then a `getty` on `/dev/ttyPHI0`.
6. Reap children; on `SIGTERM` from the host (sent through the console
   channel as an escape sequence in v1), stop services and `reboot(2)`,
   which the kernel patch series routes to an SBOX reset.

No shell scripts at boot: every step is a Rust function with an error
message that names the step, printed to the console ring.
