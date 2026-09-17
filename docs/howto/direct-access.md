# Drive the card without SSH: `phictl boot --serve` and `phictl exec`

For automation (including the assistant working in this repository) the
card is driven through a local control socket instead of SSH
(`docs/decisions/0008-direct-access-tool.md`). The process that booted the
card (the daemon) relays rpc frames (`host/crates/phi-rpc`) between the
socket and the card's agent (`card/agent`) over ring channel kind 3
(`docs/spec/ring-protocol.md`); the card end is `/dev/phirpc` (kernel patch
0022). Both ends poll at 1 kHz.

## Unprivileged, unattended (the normal way, since 2026-09-14)

With the `phi` group (from `sudo scripts/setup-arch.sh`, once) the whole
cycle runs from a normal shell:

```
scripts/phi-up.sh --toolchain --ssh  # boot in the background, wait for the agent, load clang, forward SSH
scripts/phi-run.sh nproc             # any command; stdin, stdout, stderr, exit status relayed
scripts/phi-run.sh sh -c 'cd /tmp && cc -O2 -o x x.c && ./x'
scripts/phi-down.sh                  # halt and release the card
```

The socket is `$XDG_RUNTIME_DIR/phictl/control.sock` (directory 0700,
socket 0600), the console log `$XDG_RUNTIME_DIR/phictl/console.log`, the
daemon's pid `boot.pid`. `scripts/phi-autoboot.sh install` runs the same
boot as a systemd user unit at login (`scripts/phi-autoboot.md`), with the
socket at the same path, so every command below works unchanged.

## The client commands

```
host/target/debug/phictl status                       # agent version, or an error
host/target/debug/phictl exec -- nproc                # run a program
host/target/debug/phictl exec --cwd /tmp -- sh -c 'echo $PWD; id'
echo 'main(){puts("hi");}' | host/target/debug/phictl exec -- sh -c 'cat > /tmp/x.c; wc -c /tmp/x.c'
host/target/debug/phictl put ./prog /tmp/prog         # copy in (mode kept; --mode 755 to set one)
host/target/debug/phictl exec -- /tmp/prog
host/target/debug/phictl get /tmp/out.txt ./out.txt   # copy out
host/target/debug/phictl sensors                      # answered by the daemon from the SBOX
host/target/debug/phictl traffic                      # PCIe bytes the daemon moved, by path
```

`exec` relays stdin, stdout, stderr and returns the program's exit status
(255 when the agent reports an error, for example a missing program; 128
plus the signal number when the program was killed). The socket is found
from `--socket`, else `PHICTL_SOCKET`, else `/run/phictl/control.sock`
when that exists (a root boot), else the user runtime directory
(`phictl exec --help`).

Clients and sessions (ADR 0009, 2026-09-17): the daemon accepts up to 16
connections. The card's agent runs one session at a time (a command, a
transfer, a ping), so the daemon hands the card to one client, the holder,
and the others wait their turn in connection order. `Stat` (what `phitop`
asks for), `Sensors` and `Traffic` never wait: the first is answered by the
agent from inside a running command, the other two by the daemon itself.
If the holder disconnects mid-command the daemon closes the command's
stdin and drains the session before the next client starts
(`host/crates/phictl/src/serve.md`).

## Root boot with a real network interface

```
sudo host/target/debug/phictl boot --kernel card/kernel/build/out/arch/x86/boot/bzImage --initrd card/initramfs/build/initramfs.cpio.gz --cmdline "earlyprintk=phiring console=ttyPHI0" --net phi0 --serve
```

`--net phi0` creates a TAP device (needs root and `ip` from iproute2) and
bridges the ring's network channel to it, so the card is `10.9.0.2` on the
host and SSH is `ssh root@10.9.0.2`. `--serve` without a path then creates
`/run/phictl/control.sock` owned by the user behind `sudo` (`SUDO_UID`;
override with `--owner UID`), mode 0600 in a root 0711 directory. The
console keeps running in that terminal; the banner ends with
`phi-agent on /dev/phirpc` when the agent is up. An unprivileged boot with
`--forward 2222:22` (what `phi-up.sh --ssh` passes) gives SSH on
`127.0.0.1:2222` through a userspace TCP stack inside the daemon and needs
no TAP device; that is the default path.

## What is where

- Host: `host/crates/phictl/src/serve.rs` (daemon), `client.rs`
  (commands), `host/crates/phi-rpc` (frames).
- Card: `/dev/phirpc` (kernel patch 0022), `/bin/phi-agent`
  (`card/agent`, Rust, built by `card/agent/build.sh`).
- Transport: ring channel kind 3, 256 KiB per direction, polled at 1 kHz
  on both sides; `put` moves 6 to 9 MB/s and `get` about 7 MB/s
  (`docs/results/2026-09-14-p5-net-rpc.md`, `2026-09-15-mandelbrot.md`),
  enough for sources, objects and binaries; the disk channel carries bulk
  data through the DMA engine.
- The agent and the daemon are built from one `phi-rpc` source: rebuild the
  initramfs and the host tools together after a change to the frames.
