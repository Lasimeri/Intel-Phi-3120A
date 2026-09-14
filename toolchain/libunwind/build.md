# toolchain/libunwind/build.sh

Builds LLVM's `libunwind` (from the patched `llvm-project` checkout, same
tag as the compiler) for the card and installs `libunwind.a` plus its
headers into the musl sysroot (`toolchain/build/sysroot/usr`).

Why it exists:

- Rust's `std` on `*-linux-musl` with `crt-static` links `libunwind.a`
  (`library/unwind/src/lib.rs`, the musl fallback arm) and references
  `_Unwind_Backtrace` and friends from its backtrace printer even under
  `panic = "abort"`. Without the library the final link of any Rust code
  fails.
- C++ on the card (native clang, phase P7) needs the same library under
  libc++abi.

How it is built:

- `runtimes/` CMake entry with `LLVM_ENABLE_RUNTIMES=libunwind`, compiled
  by `knc-cc` (C, assembly) and `knc-c++` (C++). libunwind includes only C
  headers, so no libc++ is required; clang warns that `-stdlib=libc++` is
  unused.
- Static only, hermetic (`LIBUNWIND_HERMETIC_STATIC_LIBRARY`), built on
  compiler-rt builtins, no cross-unwinding, no tests or docs.
- Always a clean build, for the same reason as compiler-rt: ninja does not
  rebuild objects when only the compiler changed.
- The archive is audited with `phi-isa-audit` without `--allow-suspect`;
  the x86-64 register save/restore assembly uses general registers only,
  so the result is 0 illegal, 0 suspect.

Order: after `toolchain/llvm/build.sh all`, `toolchain/musl/build.sh` and
`toolchain/compiler-rt/build.sh`, before `toolchain/rust/build-std.sh`.
