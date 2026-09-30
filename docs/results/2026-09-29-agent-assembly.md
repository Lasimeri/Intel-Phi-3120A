# 2026-09-29: the card agent in assembly

`phi-agent`, the card end of `phi run`, `put`, `get`, `status` and of
phitop's samples, was ported from Rust (`card/agent/src/*.rs`, 1028 lines)
to x86-64 assembly (`card/agent/*.S` and `defs.inc`: 4189 lines by hand, 3573
of them instructions and directives, plus the generated `errno.S`; why and
how in `card/agent/agent.md`). This records how it was checked and
what it measured against the Rust agent on a card.

Host: kernel 7.2.6-1-cachyos, GNU binutils 2.47. Card 1 (`0000:24:00.0`,
subsystem 3608, Gen2 x4 on the chipset), kernel v7.2.3 with this
repository's series, up 6 days with the initramfs of 2026-09-22, its
`phi-vpu-worker` idle throughout.

## The binary

| | Rust agent | assembly agent |
| --- | --- | --- |
| file size | 548 680 bytes (static musl, stripped) | 20 504 bytes (static, no libc, stripped) |
| instructions | | 3225, `phi-isa-audit`: 0 illegal, 0 suspect |
| threads while serving a command | 3 (reader, two pumps) | 1 |
| built with | patched libLLVM dylib, rustc with `RUSTC_BOOTSTRAP`, `build-std`, musl, libunwind | `as`, `ld` |
| sha256 (first 16) | | `47fc93aa4d448104` |

## On the host

`cargo test -p phi-rpc --test agent` (`host/crates/phi-rpc/tests/agent.md`):
15 tests, all passing, five runs in a row. The agent runs on the host
(its instructions are the KNC subset) with a socket as its device; every
reply is decoded by phi-rpc. The fixture test compares whole `StatReply`
samples field by field over six samples. As a check that the tests can
fail, dropping the steal field from the busy sum (one line of `stat.S`)
failed `stat_from_a_fixture` with cpu3 busy 14 instead of 15; restored, it
passed.

## On card 1

No restart: the running agent was replaced over SSH (dropbear rides the
ring's network channel, not the rpc channel the agent serves), then
swapped back.

```
phi -c 1 run mkdir -p /tmp/asm3
phi -c 1 put phi-agent /tmp/asm3/phi-agent
phi -c 1 sh 'kill $(pidof phi-agent); sleep 0.3; setsid /tmp/asm3/phi-agent </dev/null >/dev/null 2>/dev/console &'
```

Every verb, with the assembly agent serving:

```
$ phi -c 1 status | grep agent
agent     card agent 0.2.0 reachable through /run/user/1000/phictl/1/control.sock
$ phi -c 1 run sh -c 'echo out; echo err >&2; exit 4'; echo "exit=$?"
out
err
exit=4
$ echo hello | phi -c 1 run cat
hello
$ phi -c 1 run no-such-cmd
phictl exec: no-such-cmd: No such file or directory (os error 2)
$ time phi -c 1 run sh -c '(sleep 4; echo late) & echo early'
early
0.33 s
```

`phi -c 1 top --batch 2` drew the full frame from its samples: 228
threads, seven die temperatures, peak 75 C, 1100 MHz, 960 mV, memory,
1604 processes of which 1597 kernel threads, the process table.

### Against the Rust agent, interleaved

Three rounds, each agent in turn in each round, swapped in place as
above (`ab.sh`, below). Per agent and round: its CPU time (utime and stime
from `/proc/PID/stat`, USER_HZ 100) over 20 phitop samples at 0.2 s; wall
time of `phi -c 1 put`, `get` (compared with `cmp`, identical every time)
and a `run sha256sum` fed 16 MiB on standard input; `run true`; resident
size afterwards.

| round | agent | Stat user ticks / 20 | Stat system ticks / 20 | put 16 MiB | get 16 MiB | stdin 16 MiB | run true | VmRSS |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Rust | 102 | 210 | 1.500 s | 3.974 s | 3.440 s | 34.1 ms | 840 kB |
| 1 | asm | 32 | 261 | 1.298 s | 3.737 s | 2.553 s | 27.8 ms | 388 kB |
| 2 | Rust | 150 | 309 | 1.502 s | 3.840 s | 3.189 s | 34.3 ms | 836 kB |
| 2 | asm | 27 | 266 | 1.287 s | 3.648 s | 2.601 s | 29.0 ms | 392 kB |
| 3 | Rust | 156 | 296 | 1.377 s | 3.808 s | 3.133 s | 34.5 ms | 836 kB |
| 3 | asm | 29 | 263 | 1.293 s | 3.651 s | 2.585 s | 28.8 ms | 388 kB |

Means: a `Stat` sample costs the agent 68 ms of user time in Rust and
14.7 ms in assembly (4.7 times less); system time, the kernel generating
some 1600 /proc files per sample, is the same within noise (136 and 131
ms). put 1.460 against 1.293 s (-11 percent), get 3.874 against 3.679 s
(-5), standard input 3.254 against 2.580 s (-21), `run true` 34.3 against
28.5 ms. Every range is disjoint from the other agent's except the system
time. The transfers stay bound by the polled ring (both ends at 1 kHz),
so the gain there is the per-frame CPU on the card's in-order core, not
bandwidth.

A first run of the same comparison (same method, the binary before two
fixes) showed the assembly agent at 1348 kB resident after the transfers:
its receive buffer compacted only when a read might not fit, so 16 MiB of
`PutData` walked the whole 1.1 MiB buffer, and the input queue kept its
pages after the command. The buffer now compacts on every read, as
phi-rpc's decoder drains each frame, and the input queue is handed back
with `madvise` after each command; the table is the binary after both.

```bash
#!/usr/bin/env bash
# ab.sh ROUNDS, run from a directory holding r16 (16 MiB from /dev/urandom)
set -u
cd "$(dirname "$0")"
swap() {
    phi -c 1 sh "old=\$(pidof phi-agent); kill \$old; sleep 0.3; setsid $1 </dev/null >/dev/null 2>/dev/console & sleep 0.5" >/dev/null 2>&1
}
ticks() { phi -c 1 run sh -c 'p=$(pidof phi-agent); awk "{print \$14, \$15}" /proc/$p/stat'; }
t() { local s e; s=$(date +%s.%N); "$@" >/dev/null 2>&1; e=$(date +%s.%N); echo "$e - $s" | bc; }
for r in $(seq 1 "$1"); do
    for agent in rust asm; do
        [ $agent = rust ] && swap /bin/phi-agent || swap /tmp/asm3/phi-agent
        exe=$(phi -c 1 run sh -c 'readlink /proc/$(pidof phi-agent)/exe')
        read u0 s0 < <(ticks); phi -c 1 top --batch 20 -i 0.2 >/dev/null; read u1 s1 < <(ticks)
        put=$(t phi -c 1 put r16 /tmp/r16)
        get=$(t phi -c 1 get /tmp/r16 r16.back)
        cmp -s r16 r16.back && ok=same || ok=DIFF
        stdin=$(t sh -c 'phi -c 1 run sha256sum < r16')
        run=$(t phi -c 1 run true)
        rss=$(phi -c 1 run sh -c 'grep VmRSS /proc/$(pidof phi-agent)/status' | awk '{print $2}')
        phi -c 1 run rm -f /tmp/r16; rm -f r16.back
        echo "r$r $agent exe=$exe stat_utime=$((u1 - u0)) stat_stime=$((s1 - s0)) put=$put get=$get stdin=$stdin run=$run rss_kb=$rss get_$ok"
    done
done
swap /bin/phi-agent
```

## Deployed (2026-09-29 20:10)

`card/initramfs/build.sh` packed it (the image went from 1 407 765 to
1 163 402 bytes), then `phi -c 1 restart` and `phi -c 0 restart`. Both
cards booted with it from `/init` (`phi-agent 0.2.0 listening on
/dev/phirpc` in each card's log; `phi status` reports `card agent 0.2.0`
on both). On card 1 from that boot: exec with both streams and the
status, stdin, a missing program, 4 MiB put and get byte-identical, a
background child not holding the session (0.32 s), a phitop frame; on
card 0 exec, stdin and a frame. The shutdown path with it: `phi -c 1
down` logged `/data clean and unmounted` and POST `KH`, and `dumpe2fs -h
disk1.img` while the card was down read `Filesystem state: clean` with no
`needs_recovery`; `phi -c 1 up` brought it back. The VPU workers the
cards ran before the restart were not restarted (the co-processor
scripts start their own).
