# Build a program for the card and run it

State of the tooling this describes (2026-09-17): mainline Linux 7.2.3 with
the KNC patch series boots on the card with all 228 threads; the card has a
network (`phi0`), an SSH server, a control socket for the host tool, a
persistent disk (`/data`) and a native clang. Programs reach the card in
three ways: `phictl put` through the control socket, `scp` through the SSH
forwarder, or inside the initramfs for anything that must exist at boot.

## 1. Compile on the host

C, with the patched clang wrapper and the musl sysroot from phase P2
(`toolchain/README.md`; the wrapper removes every instruction the card
lacks and selects the knc64-x87 ABI, `docs/decisions/0002-64bit-userland-x87-abi.md`):

```
bash -c '. toolchain/env.sh && knc-cc -O2 -static -o hello hello.c'
```

Static linking is required: the card has no shared libraries. C++ is
`knc-c++` with libc++ from `toolchain/libcxx/build.sh`. Rust programs use
the target JSON plus `build-std` (`toolchain/rust/build-std.md`,
`card/agent/build.sh` is the worked example of an executable).

## 2. Check the binary

```
host/target/debug/phi-isa-audit hello       # must report 0 illegal, 0 suspect
```

An illegal instruction on the card is a `#UD` in user space (SIGILL) at
best. The initramfs build audits every ELF it packs and stops on a failure;
for files you push yourself, this step is yours.

## 3. Move it and run it

With the card up (`scripts/phi-up.sh --ssh`, or `phi.service`):

```
phi put hello /tmp/hello          # mode kept; /data/... persists across boots
phi run /tmp/hello                # stdin, stdout, stderr and the exit status relayed
phi sh /tmp/hello                 # the same thing over SSH, with a pty
scp -P 2222 hello root@localhost:/tmp/
```

`/tmp` is a tmpfs in GDDR and vanishes at power-off; `/data`, `/etc`,
`/var`, `/root`, `/home` and `/opt/phi` live on the disk image
(`scripts/phi-disk.md`), so configuration and logs persist too.
The control socket moves files at 6 to 9 MB/s, the forwarder at about
0.6 MB/s (`docs/results/2026-09-14-p5-net-rpc.md`).

Programs that must be present at boot (before any host tool connects) go
under `card/initramfs/extra/` at their final paths (`extra/opt/hello`
becomes `/opt/hello`); `card/initramfs/build.sh` audits and packs them
(`card/initramfs/extra/README.md`).

## 4. Compile on the card (phase P7)

The native toolchain (`phi-clang.tar.gz`, 84 MB, `toolchain/README.md`
steps 9 to 11) is pushed once per disk image with
`scripts/phi-up.sh --toolchain` or `card/userland/components/clang-push.sh`
(25 s) and lives in `/opt/phi`:

```
phi put prog.c /tmp/prog.c
phi run sh -c 'cd /tmp && cc -O2 -o prog prog.c && ./prog'
phi get /tmp/prog ./prog.card          # optional: audit on the host
```

`cc`, `c++`, `ld`, `ar`, `nm`, `objdump`, `strip` are in `/opt/phi/bin`
(clang 22 with lld and the LLVM tools, musl and libc++ in `/opt/phi/usr`);
their defaults come from `cc.cfg` next to the binaries, so no flags are
needed for card-correct code. `phictl exec` puts `/opt/phi/bin` first on
the PATH. C compiles are quick (0.44 s for a 228-thread pthreads program);
C++ with libc++ takes tens of seconds per file on these in-order cores
(`docs/results/2026-09-14-p7-native-clang.md`). There is no `make` on the
card yet; `card/examples/*.md` show the one-line compiles used for the
benchmarks.

## 5. The foreground alternative: `phictl boot` by hand

For kernel work, where the console matters more than the socket:

```
sudo host/target/debug/phictl boot --kernel card/kernel/build/out/arch/x86/boot/bzImage --initrd card/initramfs/build/initramfs.cpio.gz --cmdline "earlyprintk=phiring console=ttyPHI0" 2>&1 | tee ~/phi-run.log
```

About 13 s later the card prints its banner and a `~ #` prompt in the same
terminal (`sudo` is only needed for `--net`; the same line runs unprivileged
from the `phi` group). `nproc` prints 228, `cat /proc/cpuinfo` lists every
thread. `poweroff` halts the card (POST `KH`), Ctrl-C ends the session and
resets it; the card only runs while `phictl boot` runs. `--watch 0x2220500`
prints the kernel's boot marks as POST-style codes; `--serve`, `--forward`,
`--disk` and `--host-mem` add what `phi-up.sh` adds (`phictl boot --help`).

## Known limits

- Floating point: the kernel starts every task with MXCSR `0x200000` (DUE
  set); `LDMXCSR` of an image without bit 21 faults (Intel's KNC erratum),
  and musl's `fesetenv(FE_DFL_ENV)` loads `0x1f80`. Nothing built so far
  calls it; the fix (a DUE default environment in the card musl) is open
  (`docs/plan.md`, risk register).
- The console is one ring tty; a second program that wants a terminal
  shares it. Use SSH sessions (devpts is mounted) for more.
- One card session at a time on the control socket: a second `phictl exec`
  waits for the first to finish; `phitop` and `phictl sensors` do not wait
  (`docs/decisions/0009-telemetry-sideband.md`).
