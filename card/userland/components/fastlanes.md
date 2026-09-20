# fastlanes.sh

Builds [FastLanes](https://github.com/cwida/FastLanes) for the card and
routes its 32-bit `unffor` hot path through the card's 512-bit vector
unit. The kernels and the reasoning behind the port are in
[knc-fls.md](../../lib/knc-fls/knc-fls.md); this file is about the build.

```sh
bash card/lib/knc/build.sh            # libknc, which this needs
bash card/userland/components/fastlanes.sh
```

Output: `card/userland/build/fastlanes/phi-fastlanes.tar.gz`, containing
`libFastLanes.a`, `libfls_alp_primitive.a`, the headers, three programs
in `/opt/phi/bin`, and the example dataset.

The commit is pinned (`PHI_FASTLANES_COMMIT`, default
`f0edc1020a538f1f8098640fce8347c9ac247a0d`) and the tarball's SHA-256 is
in `toolchain/SHA256SUMS`. FastLanes has no tags, so a commit is the only
thing there is to pin.

GitHub generates `/archive/<sha>.tar.gz` on demand, and its bytes have
changed historically when GitHub bumped its own git version. If this
build ever stops with a SHA-256 mismatch on an unchanged commit, that is
the likely cause rather than tampering: extract both tarballs, `diff -r`
the trees, and re-pin if they are identical.

## Cross-compiling it

`toolchain/cmake/knc.cmake` is the toolchain file. Three of its settings
exist for this build in particular:

- `CMAKE_SYSTEM_PROCESSOR knc`, not `x86_64`. FastLanes and several of its
  dependencies branch on the processor name to add `-march=native` or
  `-mavx512dq`, none of which this card has.
- `CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY`. A try-compile that links
  needs a runnable target; there is nothing to run on the host.
- `-Wno-pass-failed`. ALP's encoder carries `#pragma clang loop vectorize`
  on loops this target cannot vectorise, and `-Werror` turns the resulting
  `-Wpass-failed=transform-warning` into a build failure. Upstream already
  makes the same exception for CI runners without AVX-512.

## The four patches

All applied by the script, all idempotent, all reported as they go.

**1. `#include <climits>` in `src/include/fls/cuda/common.hpp`.**
`CHAR_BIT` is used in a constant expression and reaches that file only
through a transitive include on the compilers upstream builds with. Under
this clang and libc++ it is undeclared, and the error cascades into a
variable-length-array diagnostic that `-Werror` makes fatal. Needed for
the host build too, so it is not a card issue.

**2. `last_seen_val {}` in `src/table/stats.cpp`.** The member initialiser
was `last_seen_val(0)`. The template is also instantiated for
`std::string`, where `string(0)` is `string(nullptr)` and undefined;
clang says so through `-Wnonnull`. Value initialisation is correct for
every instantiation. A real bug, not a portability shim.

**3. Rename the generated `uint32_t` dispatcher to `unffor_scalar`.** One
line in `src/alp/src/fastlanes_gen_unffor.cpp`. The generated switch over
33 widths is untouched and stays callable, which is what `fls_check`
compares against.

**4. Append `card/lib/knc-fls/fls_unffor.cpp` and link `libknc`.** The new
`unffor` checks two preconditions and calls `knc_fls_unffor`, falling back
to `unffor_scalar` otherwise. Appending it to the generated translation
unit rather than adding a source file keeps CMake out of it; the only
CMake change is one `target_link_libraries` line, appended, so every
consumer of `FastLanes` gets `-lknc`.

Patch 3 is the one that breaks loudly if upstream regenerates that file
with a different signature: the `grep` finds nothing, the rename does not
happen, and patch 4's appended definition collides with the original at
link time. That is deliberate. A patch that silently does nothing would
leave the build on the scalar path with no sign of it.

## What it checks

`fls_check` compares the MVEX `unffor` against FastLanes' scalar one for
every width 0 to 32 at every input alignment the kernels accept, and also
calls the patched dispatcher so the guard is on the same evidence.
`fls_bench` reports values per second for both, per width, on one thread.

`phi-isa-audit` runs over `libFastLanes.a` and all three programs; an
instruction the card cannot execute fails the build rather than waiting
to be a `SIGILL`.

## The example dataset

`tools/fls-example-csv.c`, compiled and run with `tcc`, writes
`/opt/phi/share/fls-example/data.csv` into the package, and the script
writes the schema next to it. `PHI_FASTLANES_ROWS` sets the row count
(default 60000, which is 59 vectors per column).

All three columns are above 65535 on purpose. FastLanes picks a physical
type by magnitude, so a column of small integers becomes `u16` and never
reaches the vector path at all. The dataset is the good case, and
`fls-example-csv.md` says so.

`fls_roundtrip` reads the first rowgroup only. At this size there is
exactly one; the loop breaks after it rather than pretending to handle a
case the measurement does not cover.

## Running it on the card

```sh
phi put card/userland/build/fastlanes/phi-fastlanes.tar.gz /tmp/fastlanes.tar.gz
phi run sh -c 'tar -xzf /tmp/fastlanes.tar.gz -C /'
phi run /opt/phi/bin/fls_check
phi run /opt/phi/bin/fls_bench
phi run /opt/phi/bin/fls_roundtrip /opt/phi/share/fls-example /tmp
```

Results are in `docs/results/2026-09-20-fastlanes-vpu.md`.
