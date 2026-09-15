# toolchain/libcxx/build.sh

libc++abi and libc++ for the card, from the same patched LLVM source as the
cross compiler, built with `knc-cc`/`knc-c++` against the musl sysroot:
static only, exceptions through the LLVM unwinder (libunwind is built again
in the same runtimes configuration, as the libc++abi option requires;
the result matches `toolchain/libunwind/build.sh`), compiler-rt for the
builtins, the ABI
library folded into `libc++.a` so that `-lc++` alone links. Installed under
the sysroot (`usr/lib/libc++.a`, `usr/lib/libc++abi.a`,
`usr/include/c++/v1`). Both archives must audit clean.

First consumer: the native clang for the card (`PHI_LLVM_VARIANT=card` in
`toolchain/llvm/build.sh`, phase P7). Runs in a few minutes.
