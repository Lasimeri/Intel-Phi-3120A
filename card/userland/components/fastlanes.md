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

## The seven patches

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

**5. Prefix-sum the candidate scan in `enc_analyze_opr`.**
`find_best_option` summed `rep_vec` over `[i, j]` on every call, and it is
called for every pair of distinct values in a vector, so the analysis was
cubic in that count: about 1.8e8 inner iterations per vector per candidate
encoding when a vector has 1024 distinct values. `AnalyzeHistogram` now
carries prefix sums, filled at the end of `Cal`, and the range sum is a
subtraction. The same sum, so the same option wins and the same bytes are
written, which is checked by comparing output files against pristine
upstream rather than asserted.

**6. Bound that scan by the exception limit it already enforces.** An
option is kept only when it leaves fewer than `LOCAL_EXC_LIMIT_C`
exceptions. `n_exceptions(i, j)` is never below `prefix[i]`, so the outer
loop stops once `prefix[i]` reaches the limit; and `n_exceptions` falls as
`j` grows, so the inner loop starts at the first admissible `j`. Every
skipped pair is one the guard would have rejected. About 400 pairs
instead of 524288 on a vector of 1024 distinct values.

**7. `Counters12::clearTouched()` in FSST12.** `buildSymbol12Map` memset
`sizeof(Counters12)` once per round, four rounds per symbol table, and
that structure is 24 MB. On the card that memset was 47.6 percent of a
real-file compression run, and it cannot be made faster: a 24 MB clear
runs at 4.82 GB/s there and a plain scalar store loop already reaches the
same figure, so the limit is the memory system. `count2Inc(pos1, pos2)`
is only reached after `count1Inc(pos1)`, and `count1High[pos1]` is
non-zero exactly when that symbol occurred, so the dirty rows of `count2`
are exactly those with `count1High[pos1] != 0`. The first round still
memsets everything, because the structure lives in an uninitialised
union; later rounds clear only what they dirtied.

Patches 5, 6 and 7 are not Knights Corner fixes. They are upstream
complexity fixes that happen to be visible here because this machine is
slow enough to make them obvious: on the host they are worth 9.1x on an
integer table and 3.16x on TPC-H lineitem, with byte-identical output.

## What it checks

`fls_check` compares the MVEX `unffor` against FastLanes' scalar one for
every width 0 to 32 at every input alignment the kernels accept, and also
calls the patched dispatcher so the guard is on the same evidence.
`fls_bench` reports values per second for both, per width, on one thread.

`fls_compress` is the encode-side measurement and reports MB/s rather
than values per second, because values per second says nothing across
columns of different widths. It takes an optional third argument that
forces a single encoding instead of letting the wizard search, which is
how the cost of the search was separated from the cost of encoding, and
it samples its own program counter when `FLS_PROF` names an output file,
because the card has neither `perf` nor `gdb`.

`phi-isa-audit` runs over `libFastLanes.a` and all four programs; an
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
phi run /opt/phi/bin/fls_compress /opt/phi/share/fls-example /tmp/example.fls
```

Results are in `docs/results/2026-09-20-fastlanes-vpu.md`.
