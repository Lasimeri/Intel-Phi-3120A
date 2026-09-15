# 2026-09-14: clang on the card (phase P7 done)

Card kernel 7.2.3 with patches 0001 to 0023, initramfs with busybox,
dropbear and `phi-agent`; the native toolchain `phi-clang.tar.gz` (84 MB,
234 MB unpacked as `/opt/phi`) built by `toolchain/libcxx/build.sh`,
`PHI_LLVM_VARIANT=card toolchain/llvm/build.sh` and
`card/userland/components/clang.sh` (host commit `5f63d0a`), loaded onto
the running card with `clang-push.sh` through the direct-access tool.

## Loading

`phictl put` moved the 84 MB tarball and `phictl exec` unpacked it into the
card's RAM and compiled a probe, 24 s in all. `/opt/phi/bin/cc --version`
on the card: `clang version 22.1.8`, target `x86_64-unknown-linux-musl`.

## Compiling on the card

Sources pushed with `phictl put`, everything below run through
`phictl exec` from an unprivileged host shell:

| on the card | compile | binary | run |
| --- | --- | --- | --- |
| `cc -O2 -o pi pi.c -lpthread` (228-thread Leibniz series, x87 doubles) | 0.44 s | 39744 bytes | `228 threads, 199999776 terms: pi = 3.141592649`, 0.24 s wall, 14.15 s CPU |
| `c++ -O2 -o t t.cpp` (vector, string, exceptions, libc++) | 16.35 s | 420576 bytes | `card c++ works caught 4.5` |

Both binaries, fetched back with `phictl get`, audit clean on the host, and
their sizes equal the ones the host cross toolchain produces from the same
sources: it is the same compiler with the same defaults (`clang.cfg` next
to the binary). The C++ compile is slow because the card's cores are
in-order at 1.1 GHz and libc++ headers are large; the C compile is quick.

## What was needed on the way

- libc++abi and libc++ for the card did not exist: `toolchain/libcxx/build.sh`
  (static, libunwind folded in, audit clean). Its probe for glibc's
  `__cxa_thread_atexit_impl` is a false positive under static-library
  try-compile and had to be forced off, or every C++ executable failed to
  link.
- The C++ wrapper `knc-c++` passed `-x c++`, which clang applied to object
  files at link time; now `--driver-mode=g++`.
- The first card clang carried 10002 illegal instructions: BLAKE3's SSE to
  AVX-512 assembly inside LLVM (`LLVM_DISABLE_ASSEMBLY_FILES=ON` builds the
  portable C version) plus seven `xgetbv` in LLVM's host-CPU detection,
  which run only after CPUID reports OSXSAVE; the package audit ignores
  XSAVE for that documented reason.
- clang's driver wants the sysroot laid out as `usr/include` and `usr/lib`,
  and compiler-rt's `crtbegin`/`crtend` objects had to travel with the
  package.
- `phictl exec` now puts `/opt/phi/bin` first on the card's PATH.

## Not done

- No `make` on the card yet (busybox has none); the LLVM tools stand in
  for binutils. gcc and tcc are phase P8, CPython and QuickJS phase P9.
- The toolchain is loaded per boot; a persistent store on the card does
  not exist.
