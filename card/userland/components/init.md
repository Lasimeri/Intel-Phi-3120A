# init

Not built as a component. PID 1 on the card is `card/initramfs/init`, a
busybox shell script; what it does, in order, is in
`card/initramfs/README.md` and the script's own comments.

## The Rust init that was planned and dropped

The original design (2026-09-13) was a Rust PID 1 built for
`x86_64-knc-linux-musl` with `-Zbuild-std`: mount the pseudo filesystems,
set `CARD_FLAG_INIT_REACHED` in the ring region header, load a `phinet`
module, bring up `phi0` from `phi.ip=`, set the clock from the region
header's `host_epoch_ns`, start dropbear and a `getty`, and reap children.

Four of those six steps stopped existing:

| Planned step | What replaced it |
| --- | --- |
| Set the init-reached flag from userspace | The kernel writes it (patch 0011, `knc_earlycon.c`), long before init runs |
| Load `phinet` | There is no module; `phi0` is kernel patch 0021 and exists before init |
| Address from `phi.ip=` | `init` takes `PHI_CARD_ADDR`, default `10.9.0.2/24`, matching `phictl boot --net` |
| `getty` on `/dev/ttyPHI0` | `console=ttyPHI0` gives init a `/dev/console`; the shell is started under `setsid` and `cttyhack` directly, which is what makes Ctrl-C work over the ring |

What remained was mounting, an address, dropbear, and supervision: about
sixty lines of shell against a Rust binary that would need
`-Zbuild-std`, a target JSON, and a place in the boot image. The shell
script won on size and on edit latency (no rebuild to change the boot
sequence). `docs/plan.md` records the deviation in the P4 row.

Rust on the card did not go away with it: `phi-agent` (`card/agent/`) is
Rust, cross-built the same way the Rust init would have been, and `init`
starts it.
