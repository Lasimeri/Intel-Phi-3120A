# toolchain/

How code for the card is compiled. Everything here follows
`docs/decisions/0002-64bit-userland-x87-abi.md` (the knc64-x87 ABI, defined
by one patched LLVM) and `docs/decisions/0007-rust-distro-rustc-patched-llvm.md`
(Rust through the distro rustc loading that LLVM).

| Entry | Contents |
| --- | --- |
| `env.sh` | Paths shared by every script: the space-free alias of the repository, the real build root under `~/.cache`, `PHI_LLVM`, `PHI_SYSROOT`, and `knc-cc` on `PATH`. Source it in `bash`. |
| `llvm/` | The LLVM/clang patch series (`llvm/patches/`; `llvm/README.md` explains every patch) and `build.sh` for the two variants: the static X86-only clang+lld that compiles C, and the `libLLVM.so` that rustc loads. |
| `clang/` | `knc-cc` and `knc-c++`, driver wrappers that bake in the flags. |
| `musl/` | musl 1.2.5 built with `knc-cc` into the sysroot, with one source patch and the drop rule for SSE assembly. |
| `compiler-rt/` | The builtins archive without SSE (patch 0007), installed into clang's resource directory. |
| `libunwind/` | LLVM libunwind for the card, installed into the sysroot; required by Rust's `std` and later by C++. |
| `rust/` | The generated target `x86_64-knc-linux-musl.json`, its generator, and `build-std.sh`. |
| `check/` | The phase P2 exit test: `hello.c` plus the `hello_rs` staticlib; `run.sh` compiles, audits, and runs it. |

## Build order (phase P2)

Every script sources `env.sh`, can be rerun, and prints what it audits.
Build products live under `~/.cache/intel-phi-3120a-build/toolchain/`
(reachable as `toolchain/build/`); `env.md` explains why those paths carry
no spaces. Times are for a 16-thread desktop.

1. `toolchain/llvm/build.sh all` (about 1 hour): patched clang and lld, X86
   only, installed to `toolchain/build/llvm/`.
2. `toolchain/musl/build.sh` (minutes): the sysroot at `toolchain/build/sysroot/`.
3. `toolchain/compiler-rt/build.sh` (minutes): builtins for `knc-cc`.
4. `toolchain/libunwind/build.sh` (a minute): `libunwind.a` in the sysroot.
5. `toolchain/check/run.sh`: the C half must print `phase P2 check: PASS`.
6. `PHI_LLVM_VARIANT=dylib toolchain/llvm/build.sh all` (about 40 minutes):
   `libLLVM.so.22.1` for rustc, installed to `toolchain/build/llvm-dylib/`.
7. `toolchain/rust/gen-target.sh` (after every rustc upgrade), then
   `toolchain/rust/build-std.sh` (under a minute): `core`, `alloc`, `std`
   and the `hello_rs` staticlib for the card, using the distro `rust` and
   `rust-src` packages. No rustup, no nightly.
8. `toolchain/check/run.sh` again: it now links the Rust half; expected
   output includes `rust_hypot=5` and an audit of the linked binary with
   zero illegal instructions.

## What the audit flags in correct output

Nothing in linked executables, by construction: `knc-cc` and the Rust
target disable every feature in the deletion list, `-nopl` with patch 0003
removes multi-byte NOP padding, `-fcf-protection=none` removes `endbr64`,
and patch 0008 turns the `pause` intrinsic into `nop`. Archives that bundle
code no program links may carry documented exceptions
(`phi-isa-audit --ignore`, see `host/crates/phi-isa-audit/src/main.md`);
today that is the single `xgetbv` in Rust's `std_detect`. A hit anywhere
else is a toolchain bug, not an audit false positive.
