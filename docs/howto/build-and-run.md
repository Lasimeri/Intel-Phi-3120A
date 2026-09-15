# Build a program on the host, run it on the card

State of the tooling this describes: mainline Linux 7.2.3 with the KNC patch
series boots on the card with all 228 threads and a busybox shell on the
ring tty (`docs/results/2026-09-14-p4-tty-smp.md`). There is no network to
the card yet (phase P5), so programs travel inside the initramfs.

## 1. Compile for the card

C, with the patched clang wrapper and the musl sysroot from phase P2
(`toolchain/README.md`; the wrapper removes every instruction the card
lacks and selects the knc64-x87 ABI, `docs/decisions/0002-*`):

```
bash -c '. toolchain/env.sh && knc-cc -O2 -static -o card/initramfs/extra/opt/hello hello.c'
```

Static linking is required: the initramfs carries no shared libraries.
Rust programs follow `toolchain/rust/README.md` (target JSON plus
`build-std`); the archive they produce links into a C `main` the same way,
`toolchain/check/run.sh` is the worked example.

## 2. Check the binary

`host/target/debug/phi-isa-audit card/initramfs/extra/opt/hello` must report
`0 illegal, 0 suspect`. The initramfs build runs this for every ELF file
under `extra/` and stops on a failure, because an illegal instruction on
the card is a silent `#UD` in user space (SIGILL) at best.

## 3. Rebuild the initramfs and boot

```
card/initramfs/build.sh
sudo host/target/debug/phictl boot --kernel card/kernel/build/out/arch/x86/boot/bzImage --initrd card/initramfs/build/initramfs.cpio.gz --cmdline "earlyprintk=phiring console=ttyPHI0" 2>&1 | tee ~/phi-run.log
```

About 13 s later the card prints its banner and a `~ #` prompt in the same
terminal. Type `/opt/hello`; output comes back through the ring. `nproc`
prints 228, `cat /proc/cpuinfo` lists every thread. `poweroff` halts the
card (POST `KH`), Ctrl-C ends the session and resets it; the card only runs
while `phictl boot` runs.

## Limits until phase P5

- No network, no file transfer other than the initramfs (rebuild takes a
  second).
- `/tmp` is a tmpfs in GDDR; nothing persists across boots.
- The console is one ring tty; a second program that wants a terminal
  shares it.
- Floating point: the kernel starts every task with MXCSR `0x200000`
  (DUE set) and `LDMXCSR` of an image without bit 21 faults; musl's
  `fesetenv(FE_DFL_ENV)` is the known caller (`docs/plan.md` risk table).

## 4. Compile on the card itself (phase P7)

With `phictl boot --serve` running (`docs/howto/direct-access.md`), load
the native toolchain once per boot and use it through `phictl exec`:

```
card/userland/components/clang-push.sh                     # 84 MB, about 25 s
host/target/debug/phictl put prog.c /tmp/prog.c
host/target/debug/phictl exec -- sh -c 'cd /tmp && cc -O2 -o prog prog.c && ./prog'
host/target/debug/phictl get /tmp/prog ./prog.card          # optional: audit on the host
```

`cc`, `c++`, `ld`, `ar`, `nm`, `objdump`, `strip` live in `/opt/phi/bin`
(clang 22 with lld and the LLVM tools, musl and libc++ in `/opt/phi/usr`);
their defaults come from `clang.cfg` next to the binaries, so no flags are
needed for card-correct code. C compiles are quick; C++ with libc++ takes
tens of seconds per file on these cores. The same works over SSH once
logged in.
