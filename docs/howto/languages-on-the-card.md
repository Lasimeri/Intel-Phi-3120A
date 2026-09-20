# Five languages on the card, with the vector unit

C, C++, Rust, JavaScript and Python all run on the Xeon Phi 3120A, and all
five reach its 512-bit vector unit. This is how, and what each one costs.

Everything below was verified on 2026-09-20; the numbers are decode
throughput at 11 bits per value, one thread, from
`docs/results/2026-09-20-languages.md`.

## The short version

| Language | On the card | Vector unit | 11-bit decode |
| --- | --- | --- | --- |
| C | `cc` (clang 22.1.8), natively | `#include <knc.h>`, `-lknc` | 403.9 M/s |
| C++ | `c++`, libc++, natively | `#include <knc.hpp>`, `-lknc` | same library |
| Rust | cross-compiled from the host | the `knc` crate | 270.2 M/s |
| JavaScript | `qjs` (QuickJS 2026-06-04) | `import * as knc from "knc"` | 398.9 M/s |
| Python | `python3` (CPython 3.14.7) | `import knc` | 395.7 M/s |

For scale: the same work in scalar C on the same machine manages 92.6 M/s
over this layout, and 37.8 M/s over an ordinary contiguous bitstream. So
the vector unit is worth about 4.4x over the card's best scalar code, and
the layout is worth 2.5x before any vector code at all. Both numbers
matter; quoting only the 37.8 would credit the vector unit with the
layout's share.

## Why there is one library and not five bindings

No compiler emits Knights Corner vector instructions. Not clang, not rustc,
not the card's own clang, nothing since Intel's dead k1om toolchain. There
are no intrinsics, no `core::arch`, no auto-vectorisation, and the project's
patched LLVM adds only the *absence* of instructions the card lacks (ADR
0002, ADR 0007).

So the kernels are hand-encoded, once, by `host/crates/knc-mvex`, and shared
through `libknc` (`card/lib/knc/knc.md`). Every language reaches the same
`.a` file. Nothing is duplicated and nothing is auto-vectorised, because
nothing can be.

## C

Native. The card's clang has `--sysroot=/opt/phi` built into its config, so
headers and libraries are found without flags.

```sh
card/lib/knc/build.sh
phi put ~/.cache/intel-phi-3120a-build/userland/libknc/phi-libknc.tar.gz /tmp/libknc.tar.gz
phi run sh -c 'tar -xzf /tmp/libknc.tar.gz -C /'
phi put prog.c /tmp/prog.c
phi run sh -c 'cd /tmp && cc -O2 -o prog prog.c -lknc && ./prog'
```

Or cross-compile on the host with `knc-cc -O2 prog.c -lknc`, which links
the same library out of the sysroot.

## C++

Native, C++17, libc++ (`-stdlib=libc++` is in the card's clang config).
`knc.hpp` adds `knc::codec`, `knc::aligned_buffer` and range-checked entry
points over the C ABI.

```sh
phi run sh -c 'cd /tmp && c++ -std=c++17 -O2 -o prog prog.cpp -lknc -lpthread && ./prog'
```

`std::thread` works; `card/lib/knc/knc_bench.cpp` uses 228 of them.

## Rust

**Cross-compiled, and that is the standard install for this target.** There
is no `rustc` on the card and building one would mean cross-compiling rustc
itself against the card's LLVM, which is a project of its own scale rather
than a step here. The card's own agent is Rust and is built this way.

```sh
card/lib/knc-rs/build.sh
phi put ~/.cache/intel-phi-3120a-build/userland/knc-rs/knc-demo /tmp/knc-demo
phi run sh -c 'chmod +x /tmp/knc-demo && /tmp/knc-demo 228 64 256'
```

Your own crate needs `.cargo/config.toml` pointing at
`toolchain/rust/x86_64-knc-linux-musl.json` with `-Zbuild-std`, and
`knc = { path = "../../card/lib/knc-rs" }`. `card/lib/knc-rs/build.md` has
the three environment settings that make it work and why each is there.

One warning per crate is expected: "target feature `sse2` must be enabled to
ensure that the ABI of the current target can be implemented correctly".
ADR 0007 explains it.

## JavaScript

Native. `qjs` is QuickJS 2026-06-04, built static with the `knc` module
compiled in, so no loader and no path are involved:

```js
import * as knc from "knc";
const buf = knc.alloc(knc.BLOCK * 4);
```

```sh
card/userland/components/quickjs.sh
phi put ~/.cache/intel-phi-3120a-build/userland/quickjs/phi-quickjs.tar.gz /tmp/q.tar.gz
phi run sh -c 'tar -xzf /tmp/q.tar.gz -C /'
phi run qjs --std /tmp/script.js
```

`qjsc` is there too, so a script can be compiled to a C file and linked into
a standalone binary.

## Python

Native. CPython 3.14.7, one static interpreter with every module the
sysroot supports built in.

```python
import knc
buf = knc.Buffer(knc.BLOCK * 4)
```

```sh
PHI_PYTHON_EXTRA_SRC=card/lib/knc-py PHI_PYTHON_SETUP=card/lib/knc-py/Setup.local \
  card/userland/components/cpython.sh
phi put ~/.cache/intel-phi-3120a-build/userland/cpython/phi-python.tar.gz /tmp/p.tar.gz
phi run sh -c 'tar -xzf /tmp/p.tar.gz -C /'
phi run python3 /tmp/script.py
```

**There is no `ctypes`, and there cannot be.** Static musl has no `dlopen`,
so `ctypes.CDLL` has nothing to load even with libffi ported. That is why
`knc` is compiled into the interpreter rather than sitting beside it as a
`.so`, and why `PHI_PYTHON_SETUP` exists. The same is true of every other
extension module: if it is not built in, it is not available.

## All five carry the whole library

Bit packing at every width from 1 to 32 in both directions, the frame of
reference and delta transforms on top of it, and the block copy. Each
binding spells the names its own way (`unpack_for` in C, Python and Rust,
`unpackFor` in JavaScript, `knc::codec::unpack_for` in C++) and each one's
demo checks all 32 widths of all three decode paths against a scalar model
written in that language.

## The two rules every binding follows

**Buffers must be 64-byte aligned.** Every kernel loads and stores whole
512-bit vectors and there is no unaligned form. Each language provides the
way to get such a buffer, because none of their default allocators do:
`knc::aligned_buffer` in C++, `Block` and `Packed` in Rust, `knc.Buffer` in
Python, `knc.alloc` in JavaScript, `posix_memalign` in C.

**Call across the boundary once per buffer, not once per block.** All four
non-C bindings decode every whole block the buffers hold in a single call,
which is why Python and JavaScript land within 2 percent of C. A
per-block call from an interpreter would cost more than the kernel.

## Persistence

`/opt/phi` is a bind mount of a directory on `/data`, which is the
host-backed block device, so anything installed there survives a reboot.

`card/initramfs/init` does two separate things that both matter here. It
sets `PATH=/opt/phi/bin:/bin:/sbin:/usr/bin:/usr/sbin` for everything it
starts, which is why a freshly unpacked `qjs` is found without a reboot.
And it rebuilds a symlink in `/usr/bin` for every executable under
`/opt/phi/bin` on each boot, so a non-login shell finds them too: dropbear
hands those a bare `/usr/sbin:/usr/bin:/sbin:/bin` and never reads
`/etc/profile`, so without the links `ssh phi python3` would fail while an
interactive `ssh phi` worked.

## What is not here

`node`, `bun`, `deno`: all need SSE2 at minimum and a JIT that emits SSE
(ADR 0004). `perl`, `lua`, `tcc`: not ported, no obstacle known.
`rustc` and `cargo` on the card: see above.
