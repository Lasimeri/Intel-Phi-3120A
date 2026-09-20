# knc.cmake

A CMake toolchain file for the card, so a project with a CMake build can be
cross-compiled without being taught about this one.

```sh
cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE="$PWD/toolchain/cmake/knc.cmake" \
      -DCMAKE_BUILD_TYPE=Release
```

It sets `knc-cc` and `knc-c++` as the compilers, points `CMAKE_SYSROOT` at
`PHI_SYSROOT` (or the default under `toolchain/build`), and forces static
libraries, since the card has no dynamic loader.

Two settings are less obvious and both are there for a reason.

**`CMAKE_SYSTEM_PROCESSOR` is `knc`, not `x86_64`.** Projects branch on it.
FastLanes, for one, adds `-march=native` when it sees an x86 processor
without AVX-512DQ, and `-mavx512dq` when `/proc/cpuinfo` shows the host has
it. Both are wrong for this target: `native` describes the machine doing the
compiling, and AVX-512 is a different 512-bit encoding that this card does
not implement (`docs/decisions/0002`). `knc` falls through to "no
instruction set flags applied", which is correct, because `knc-cc` already
names the architecture and every extension the card lacks.

**`CMAKE_TRY_COMPILE_TARGET_TYPE` is `STATIC_LIBRARY`.** `knc-cc` does not
add `-static`, because the kernel build needs it not to. A probe program
that linked would therefore be a dynamic executable, which nothing on the
card can load. CMake would still report success, so the failure would not
appear until much later and somewhere else. Compiling the probes to an
archive asks the question that can actually be answered here.

Used by `card/userland/components/fastlanes.sh`.

## The third setting: `-Wno-pass-failed`

Not an obvious one either. No loop on this card can be vectorised by the
compiler, because LLVM has no Knights Corner vector backend at all, so
`#pragma clang loop vectorize(enable)` always fails its transformation and
clang says so. That is advisory, but a project built with `-Werror` makes
it fatal, and the project is not wrong to: on a machine where the pragma
means something, a failed vectorisation is worth stopping for.

FastLanes hits it in ALP's encoder, and its own CMakeLists already carves
out the same exception for CI runners that lack AVX-512. Here it is not a
CI quirk, it is the permanent state of the target, so the toolchain file
carries it rather than each project.

It is also the clearest statement of why `libknc` exists: the compiler is
reporting, at build time, that FastLanes' entire portability model (write
scalar, let the vectoriser do the rest) has nothing to work with here.
