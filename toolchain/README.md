# toolchain/

How code for the card is compiled. Everything here follows
`docs/decisions/0002-64bit-userland-x87-abi.md` (the knc64-x87 ABI, defined
by one patched LLVM) and `docs/decisions/0007-rust-distro-rustc-patched-llvm.md`
(Rust through the distro rustc loading that LLVM).

| Entry | Contents |
| --- | --- |
| `env.sh` | Paths shared by every script: the space-free alias of the repository, the real build root under `~/.cache`, `PHI_LLVM`, `PHI_SYSROOT`, and `knc-cc` on `PATH`. Source it in `bash`. `env.md` explains why the build trees are not in the source tree. |
| `fetch.sh` | `phi_fetch NAME URL`: downloads a tarball into the cache once and checks it against `SHA256SUMS`. The six component scripts that download a tarball all go through it, so no build depends on whatever upstream serves today. LLVM is pinned differently: `llvm/build.sh` does a depth-1 clone of the tag in `PHI_LLVM_TAG`, `llvmorg-22.1.8`. |
| `SHA256SUMS` | The six pinned tarballs: musl 1.2.5, busybox 1.37.0, dropbear 2025.88, zlib 1.3.1, ncurses 6.5, Python 3.14.7. |
| `llvm/` | The nine-patch LLVM/clang series (`llvm/patches/`, `llvm/README.md` explains each one) and `build.sh` for three variants: the default static X86-only clang+lld that compiles C for the card, `dylib` (`libLLVM.so` for rustc to load), and `card` (the same compiler rebuilt to run on the card). |
| `clang/` | `knc-cc` and `knc-c++`, driver wrappers that bake in the feature flags so no caller can forget them. |
| `musl/` | musl 1.2.5 built with `knc-cc` into the sysroot, with one source patch (`patches/0001-x86_64-a_spin-without-pause.patch`) and the drop rule for SSE assembly. |
| `compiler-rt/` | The builtins archive without SSE (LLVM patch 0007), installed into clang's resource directory. |
| `libunwind/` | LLVM libunwind for the card, installed into the sysroot; required by Rust's `std` and by C++. |
| `libcxx/` | `libc++.a` and `libc++abi.a` against musl, needed to build clang itself for the card. |
| `rust/` | The generated target `x86_64-knc-linux-musl.json`, its generator, and `build-std.sh`. |
| `check/` | The phase P2 exit test: `hello.c` plus the `hello_rs` staticlib; `run.sh` compiles, audits, and runs it. |

## Build order

Every script sources `env.sh`, can be rerun, and audits what it produces
with `phi-isa-audit`. Build products live under
`~/.cache/intel-phi-3120a-build/toolchain/` (reachable as
`toolchain/build/`). Durations are measured on a 16-thread desktop and
recorded in `docs/results/2026-09-13-p2-*.md`; the same table with its
result column is `docs/reproducibility.md` section 6.

| Step | Command | Time |
| --- | --- | --- |
| 1 | `toolchain/llvm/build.sh all` | 55 min first time, 15 min clean rebuild |
| 2 | `toolchain/musl/build.sh` | minutes |
| 3 | `toolchain/compiler-rt/build.sh` | 2 min |
| 4 | `toolchain/libunwind/build.sh` | 1 min |
| 5 | `toolchain/check/run.sh` | seconds; must print `phase P2 check: PASS` |
| 6 | `PHI_LLVM_VARIANT=dylib toolchain/llvm/build.sh all` | 12 min clean |
| 7 | `toolchain/rust/gen-target.sh`, then `toolchain/rust/build-std.sh` | under a minute |
| 8 | `toolchain/check/run.sh` again | seconds; now links the Rust half, expects `rust_hypot=5` |
| 9 | `toolchain/libcxx/build.sh` | a few minutes |
| 10 | `PHI_LLVM_VARIANT=card toolchain/llvm/build.sh configure`, then `build` | not recorded |
| 11 | `card/userland/components/clang.sh` | not recorded; produces `phi-clang.tar.gz`, 84 MB |

Steps 1 to 8 are what the kernel and the initramfs need. Step 6 needs the
distro `rust` and `rust-src` packages; step 7 is rerun after every rustc
upgrade. Steps 9 to 11 exist only to compile on the card itself (phase
P7); `card/userland/components/clang-push.sh` then loads the result onto a
running card without SSH.

One `sse2` warning per crate during step 7 is expected output, not a
failure: "target feature `sse2` must be enabled to ensure that the ABI of
the current target can be implemented correctly"
(rust-lang/rust#116344). The patched LLVM implements that ABI without
SSE2, which is the whole point; ADR 0007 records the measurement and the
escape hatch for the day the warning becomes an error.
## What the audit flags in correct output

Nothing in linked executables, by construction: `knc-cc` and the Rust
target disable every feature in the deletion list, `-nopl` with patch 0003
removes multi-byte NOP padding, `-fcf-protection=none` removes `endbr64`,
and patch 0008 turns the `pause` intrinsic into `nop`. Archives that bundle
code no program links may carry documented exceptions
(`phi-isa-audit --ignore`, see `host/crates/phi-isa-audit/src/main.md`);
today that is the single `xgetbv` in Rust's `std_detect`. A hit anywhere
else is a toolchain bug, not an audit false positive.
