# 2026-09-29: phictl ported to assembly

`phictl`, the host's control of the cards (opening a card through VFIO,
booting it, serving its console, control socket, disk, host memory and
SSH forward, and the client verbs), was ported from Rust
(`host/crates/phictl` with `phi-vfio`, `phi-hw`, `phi-ring` and
`phi-regs`, about 8000 lines, and the smoltcp crate for the forwarder)
to x86-64 assembly: `host/asm/phictl`, 17 sources and `defs.inc`, 12689
lines of which 11300 are instructions and directives, built with GNU
`as` and `ld` alone (a daemon that is running from the installed path is
replaced by rename, so `make build` works while cards are up). This records what was
verified against the Rust program before it was removed, what was
measured, and what changed in the design. The card agent's port is the
record before this one.

Host: kernel 7.2.6-1-cachyos, binutils 2.47, a 5800X (AVX2, no AVX-512).
Card 1 (`0000:24:00.0`, subsystem 3608, Gen2 x4 on the chipset) for every
live test, booted by the program under test; card 0 kept its Rust daemon
for the client tests against it. Same kernel and initramfs for both
programs.

## Verified, subcommand by subcommand

| what | how | result |
| --- | --- | --- |
| `cards`, `cards --plain` | `diff` against the Rust tool's output | byte-identical |
| `info`, `regs` | the Rust tool then the assembly one on card 1 (down); each open resets the card, so the POST code and SPAD2 differ by timing | every other line identical; `regs` byte-identical |
| `reset` | the assembly tool | POST trace 30, 3C, 3F, 16, 09, 0F, 10, 12; ready after 9.28 s; SPAD2 0x040001c1 |
| `fill` | 1 MiB and 16 MiB with the chunked read-back | no mismatch (a `poke` cannot be checked by a later `peek`: the open in between resets the card) |
| `boot` | the same kernel, initramfs and command line as the unit's; the report lines against the Rust daemon's journal of the morning's boot | identical addresses and sizes (image 8750080 bytes at 0x4000000, cmdline at 0x4858400, initramfs 1163402 bytes at 0x8000000, ring 16 MiB at 0x10000000); K0 to K7, `smp: Brought up 1 node, 228 CPUs`, the console tailed |
| the relay (`--serve`) | the Rust client and phitop against the assembly daemon | `phi status` (agent 0.2.0), exec with stdin, stderr and the exit status, 4 MiB put and get byte-identical, 4 MiB through stdin, a phitop frame while `sleep 3` held the card, a second client queued and served |
| the client verbs | the assembly client against the Rust daemon on card 0, then against the assembly daemon on card 1 | exec (stdin, stderr, status 7, `--cwd`), put with `--mode 640`, get byte-identical (300000 bytes), sensors and traffic in the Rust format |
| `--disk`, `--host-mem` | a 4 GiB scratch image and 2 GiB of host memory on card 1 | DMA self-test 1652 MB/s on both channels; `/data` mounted, phiblk1 the card's swap, `/dev/phihost` present; 64 MiB written and read back with sha256 equal; host memory with swap off, 32 MiB in and out byte-identical; `/data clean and unmounted` on shutdown, the image clean |
| `--forward` | ssh to the card's dropbear through the assembly TCP stack | `uname`, 16 MiB up and 16 MiB down byte-identical, two sessions at once; 300000 random bytes to a listener on the card and back, byte-identical (a tcc client, `tcpcat.c`, the host has no netcat) |
| `--net` | not exercised: the units run unprivileged and use `--forward` | |

Not carried over: `PHICTL_DMA_STRESS` (a self-test of the engine with
random copies, the engine's behaviour being recorded in the DMA results
of 2026-09-16).

## Measured

Every rate below is card 1 (Gen2 x4), the two programs interleaved on
the same boot images.

| path | Rust | assembly |
| --- | --- | --- |
| aperture write, 16 MiB, 4 KiB chunks with read-back (`phictl fill`) | 0.560 s, 0.582 s | 0.088 s, 0.087 s |
| the same without read-back | 0.570 s | 0.063 s |
| ssh upload, 16 MiB | about 0.6 MB/s (2026-09-15) | 4.41 s (3.8 MB/s) |
| ssh download, 16 MiB | | 4.46 s (3.8 MB/s) |
| `phi put` 16 MiB / 4 MiB over the rpc ring | 1.29 s | 0.34 s for 4 MiB (the same 12 MB/s) |
| `phi get` | 3.68 s for 16 MiB | 1.00 s for 4 MiB (the same 4 MB/s) |
| `/data` read after a cache drop, 64 MiB | 326 MB/s | 350 MB/s |
| `/data` 64 MiB of zeros written and synced | 196 MB/s | 195 MB/s |
| `/dev/phiblk1` read, 32 MiB | 170 MB/s | 159 MB/s |
| `/dev/phiblk1` write, 32 MiB (swap off) | | 130 MB/s |

The aperture write path is 6.5x to 9x faster: the Rust
`Mapping::write_bytes` issued 8-byte volatile stores, the assembly issues
32-byte AVX2 stores, one PCIe write transaction where four were needed.
It is the loader's path (the 10 MB of images at boot) and the aperture
fallback of the block service. The ssh forward is about 6x faster
because the stack runs in the daemon's loop and answers every segment
in the same pass; the earlier download of 32.96 s (0.5 MB/s) was the
stack advertising a zero window without ever updating it once the host
drained the buffer, so Linux paced itself with zero-window probes; a
window update after delivery fixed it. Everything on the rpc ring and
the DMA paths is the same speed: the rings are bound by the card's
polling and the engine by the link, not by the host program.

## What changed in the design

- One thread instead of five. The Rust daemon ran the console, the
  relay, each block service and the forwarder on their own threads;
  the assembly daemon gives each a turn per pass, spins for 20 ms after
  activity and then sleeps 200 us between passes. No lock exists. The
  price is that a blocking call in one service (the image's `pread`,
  `pwrite` and `fdatasync`) stalls the others for its duration.
- Backpressure on the relay: bytes for the card wait in a 4 MiB buffer
  and a client is not read while it is within two frames of full (the
  Rust `Vec` grew without bound).
- The forwarder's TCP stack is a client-only stack of about 1100 lines
  (`host/asm/phictl/tcp.md` lists what it has and what it lacks:
  no congestion control, no SACK, no reassembly, no window scaling);
  smoltcp is gone.
- `reset` logs each POST change as it happens rather than listing them
  at the end.

## Defects met during the port, for the record

- The SYN carried an MSS option but the IP total length was computed
  before the TCP header was built: the card dropped every SYN.
- Two helpers that compute a buffer's address clobber `rcx` and `rdx`;
  three callers used a length in `rcx` afterwards. One of them delivered
  160 bytes to the host socket whatever was buffered and ran the receive
  head past its tail, after which the connection delivered zeros
  forever: a 13.98 GB file in seconds, which filled the 16 GB tmpfs the
  session's scratchpad lived on and took the tool's output down with it.
  Test outputs are capped now and test logs live on disk.
- The Internet checksum's odd-byte test read a clobbered register.
- A `dd` to `/dev/phiblk1` "failed" the host memory test until the same
  test under the Rust daemon showed `Text file busy`: the device is the
  card's active swap. With `swapoff` first both daemons pass.

## Deployed

`make build` installs the binary at `host/target/debug/phictl`, the path
`scripts/phi-env.sh` and the family resolve; the units boot the cards
with it from the next `phi up`. Both cards were restarted under it on
2026-09-29 (`phi status`: agent reachable on both).
