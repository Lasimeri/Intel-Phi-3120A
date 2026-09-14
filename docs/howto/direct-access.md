# Drive the card without SSH: `phictl boot --serve` and `phictl exec`

For automation (including the assistant working in this repository) the
card is driven through a local control socket instead of SSH
(`docs/decisions/0008-direct-access-tool.md`).

## Start the card with the control socket (root, once per session)

```
sudo host/target/debug/phictl boot --kernel card/kernel/build/out/arch/x86/boot/bzImage --initrd card/initramfs/build/initramfs.cpio.gz --cmdline "earlyprintk=phiring console=ttyPHI0" --net phi0 --serve
```

`--serve` creates `/run/phictl/control.sock` owned by the user behind
`sudo` (`SUDO_UID`; override with `--owner UID`), mode 0600 in a root
0711 directory. `--net phi0` is optional (it also gives SSH). The console
keeps running in that terminal; the banner ends with
`phi-agent on /dev/phirpc` when the agent is up.

## Use it from an unprivileged shell of the owner

```
host/target/debug/phictl status                       # agent version
host/target/debug/phictl exec -- nproc                # run a program
host/target/debug/phictl exec --cwd /tmp -- sh -c 'echo $PWD; id'
echo 'main(){puts("hi");}' | host/target/debug/phictl exec -- sh -c 'cat > /tmp/x.c; wc -c /tmp/x.c'
host/target/debug/phictl put ./prog /tmp/prog         # copy in (mode kept)
host/target/debug/phictl exec -- /tmp/prog
host/target/debug/phictl get /tmp/out.txt ./out.txt   # copy out
```

`exec` relays stdin, stdout, stderr and returns the program's exit status
(255 when the agent reports an error, for example a missing program). One
client at a time; a second client waits for the socket to be free.

## What is where

- Host: `host/crates/phictl/src/serve.rs` (daemon), `client.rs`
  (commands), `host/crates/phi-rpc` (frames).
- Card: `/dev/phirpc` (kernel patch 0022), `/bin/phi-agent`
  (`card/agent`, Rust, built by `card/agent/build.sh`).
- Transport: ring channel kind 3 (`docs/spec/ring-protocol.md`), polled
  at 1 kHz on both sides; throughput of a few MB/s, enough for sources,
  objects and binaries.
