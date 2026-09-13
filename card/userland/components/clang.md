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
