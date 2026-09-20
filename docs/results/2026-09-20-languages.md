# 2026-09-20: five languages on the card, all reaching the vector unit

C, C++, Rust, JavaScript and Python now all run on the Xeon Phi 3120A and
all five reach its 512-bit vector unit through `libknc`. `docs/howto/languages-on-the-card.md`
is the working document; this records what was measured and what had to be
fixed.

## State before and after

| Language | Before | After |
| --- | --- | --- |
| C | clang 22.1.8 on the card, no vector access | `#include <knc.h>`, `-lknc` |
| C++ | clang, libc++, verified working, no vector access | `knc.hpp` |
| Rust | cross-compiles (ADR 0007), no vector access | the `knc` crate |
| Python | CPython 3.14.7 on the card, no `_ctypes`, no route to compiled code | `knc` built into the interpreter |
| JavaScript | **absent entirely** | QuickJS 2026-06-04 with `knc` compiled in |

## Decode throughput, 11 bits, one thread

Millions of values per second, unpacking 64 blocks per call.

| Path | M/s | Against C |
| --- | --- | --- |
| C, `libknc` | **403.9** | 1.00 |
| JavaScript, `qjs` | 398.9 | 0.99 |
| Python, `python3` | 395.7 | 0.98 |
| Rust, `knc` crate | 270.2 | 0.67 |
| C, scalar over the ordinary layout | 37.8 | 0.09 |

The interpreters land within 2 percent of C because the call crosses the
language boundary once per buffer rather than once per block: 64 blocks of
1024 values per call, with the GIL released in Python's case. Rust is lower
for a different and documented reason, that its `Packed` type is 4 KiB at
every width, so at 11 bits the working set is three times larger than it
needs to be and the memory bound arrives sooner
(`card/lib/knc-rs/src/lib.md`).

Every one of the five checks all 32 bit widths against a scalar model of
the layout written in that language, not only against the opposite kernel.
All passed on the first run in each language.

## Three bugs found

**QuickJS linked a dynamic PIE.** `knc-cc` does not add `-static` (the
kernel build needs it not to) and QuickJS's Makefile does not either. The
failure was `relocation R_X86_64_64 cannot be used against symbol
'knc_unpack_b1'; recompile with -fPIC`, from `libknc`'s function tables,
which is a misleading message: the fix is not `-fPIC`, it is not building a
PIE. The card has no dynamic loader, so the binary could never have run.

**QuickJS used PAUSE.** `Atomics.pause()` reaches for it through inline
assembly guarded on `__x86_64__`, and KNC deletes PAUSE. Adding
`&& defined(__SSE2__)` to the guard sends this card to the empty no-op the
file already has for other architectures.

This is the **third dialect of the same portability bug** in this project,
and worth naming as a pattern:

| Project | Dialect |
| --- | --- |
| xz | inline assembly guarded on `__x86_64__` as a proxy for "has CMOV" |
| zstd | `__attribute__((target("lzcnt,bmi,bmi2")))` on function bodies with CPUID dispatch |
| QuickJS | inline assembly guarded on `__x86_64__` as a proxy for "has PAUSE" |

All three assume x86-64 implies a feature set. None is visible in a build
log. All three produce a binary that links cleanly and faults at run time.
The ISA audit is what catches them, which is the argument for auditing every
linked executable with no exceptions.

**A buffer protocol mistake of my own.** The Python `knc.Buffer` derived
`readonly` from `PyBUF_WRITABLE`, which is the obvious reading of
`PyBuffer_FillInfo` and is wrong: a writable object reports `readonly = 0`
whatever the caller asked for, the way `bytearray` does. Deriving it makes a
plain `memoryview(buf)` read-only and every assignment through it fail. Found
by exercising the module on the host against scalar stand-ins for the
kernels before spending a CPython rebuild on it.

## Two things that are structural, not missing

**Python has no `ctypes` and cannot.** Static musl has no `dlopen`, so
`ctypes.CDLL` has nothing to load even with libffi ported to the knc64-x87
ABI. A built-in module is the only route from Python to compiled code on
this machine, which is why `cpython.sh` has `PHI_PYTHON_SETUP` and
`PHI_PYTHON_EXTRA_SRC` at all.

**Rust is cross-compiled, and that is the standard install here.** There is
no `rustc` on the card. Building one means cross-compiling rustc itself
against a card build of LLVM, which is a project of its own scale rather
than a step in this one. The card's agent has been Rust built this way since
2026-09-13 (ADR 0007).

## Sizes

| Package | Compressed |
| --- | --- |
| `phi-python.tar.gz` | 20.1 MB (60 MB installed) |
| `phi-quickjs.tar.gz` | 6.2 MB |
| `phi-libknc.tar.gz` | 24 kB |

`/opt/phi` is a bind mount of a directory on `/data`, the host-backed block
device, so all of this survives a reboot.
