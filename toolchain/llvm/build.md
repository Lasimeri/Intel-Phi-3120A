# toolchain/llvm/build.sh

One script from pristine sources to an installed compiler, so the result
is the same on any Arch host.

| Step | What |
| --- | --- |
| `fetch` | Shallow clone of `llvm-project` at `PHI_LLVM_TAG` (default `llvmorg-22.1.8`, the version Arch ships, so the host clang can build it) into `toolchain/build/llvm-project` |
| `patch` | Reset the tree to the tag, then `git apply --index` each patch listed in `patches/SERIES` |
| `configure` | cmake, Ninja if present (`scripts/setup-arch.sh` installs it), X86 only, `clang;lld`, Release, no tests/docs/bindings, default triple `x86_64-unknown-linux-musl`, defaults to lld, compiler-rt, libc++ |
| `build` | `cmake --build -j$(nproc)`; about an hour on a 5800X |
| `install` | into `toolchain/build/llvm`, which is what `knc-cc` uses by default |
| `check` | compiles the two probe files from the ABI research with `knc-cc`, audits them, and shows that `d.c` returns through `fld`/`fstp` |
| `all` | all of the above |

Environment overrides: `PHI_LLVM_TAG`, `PHI_LLVM_SRC`, `PHI_LLVM_BUILD`,
`PHI_LLVM`, `PHI_JOBS`.

## Why these cmake options

- `LLVM_TARGETS_TO_BUILD=X86`: halves build time; no other target is ever
  needed.
- `LLVM_DEFAULT_TARGET_TRIPLE=x86_64-unknown-linux-musl`: the card triple,
  so a bare `clang` from this install already targets musl; `knc-cc` still
  passes it explicitly.
- Runtimes (`compiler-rt`, `libunwind`, `libc++`) are not built here: they
  must be compiled *with* `knc-cc` against the musl sysroot, which does not
  exist until musl is built. They are a second cmake invocation in phase
  P2's runtime step.
- `LLVM_PARALLEL_LINK_JOBS=4`: linking clang with 16 parallel jobs exhausts
  64 GiB.

## Disk and time

Source about 2 GB, build directory about 10 GB, install about 1.5 GB.
