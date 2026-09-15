# clang on the card

The first native compiler: the same patched LLVM as the cross toolchain,
rebuilt with the cross toolchain for the card (a Canadian cross), with only
the X86 target, `clang` and `lld`, `LLVM_ENABLE_PROJECTS=clang;lld`,
`-DLLVM_TARGETS_TO_BUILD=X86`, static libc++ built against musl.

Default flags for the card's `cc` are baked in through a config file
(`clang` reads `<prefix>/etc/clang/x86_64-unknown-linux-musl.cfg`), so a
plain `cc hello.c` on the card produces knc64-x87 code. The audit tool is
also cross-built for the card so a user can check their own binaries.

Binary size is the cost: several hundred MB in the initramfs. It ships in a
separate squashfs image loaded on demand rather than in the base initramfs.

## Build and use (2026-09-14)

1. `toolchain/libcxx/build.sh`: libc++abi and libc++ for the card into the
   sysroot (static, libunwind folded in).
2. `PHI_LLVM_VARIANT=card toolchain/llvm/build.sh configure` then `build`:
   clang, lld and the binutils-style tools built for the card with
   knc-cc/knc-c++ (Canadian cross, tablegen from the host build), static on
   musl and libc++, X86 only. Not installed; the binaries stay in
   `~/.cache/intel-phi-3120a-build/toolchain/llvm-build-card/bin`.
3. `clang.sh`: packages them with the sysroot (musl headers and libraries,
   libc++, the compiler headers and compiler-rt) as `/opt/phi` in
   `card/userland/build/clang/phi-clang.tar.gz`, audits the tools, and
   proves the packaged clang by building and running a probe on the host
   (card binaries run on the host).
4. `clang-push.sh`: with `phictl boot --serve` running, pushes the tarball
   through the control socket, unpacks it into the card's RAM and compiles
   a probe there. `/opt/phi/bin/cc file.c` on the card then produces code
   the card runs; the defaults come from `clang.cfg` next to the binary
   (the host wrapper's flags, `--sysroot=/opt/phi`, static, compiler-rt,
   libc++).

The tarball is loaded per boot (the card has no persistent storage);
a few hundred MB through the rpc channel take well under a minute.
