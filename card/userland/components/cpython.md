# CPython

Cross-build of CPython 3.13 or newer against musl with `knc-cc`:

```
./configure --host=x86_64-unknown-linux-musl --build=x86_64-pc-linux-gnu \
    --with-build-python=/usr/bin/python3 --disable-ipv6 --with-ensurepip=install \
    --enable-optimizations=no ac_cv_file__dev_ptmx=yes ac_cv_file__dev_ptc=no
```

(`--with-build-python` is the host's interpreter, used only to run the
build's own scripts, which is the one place this project executes Python
on the host: it is CPython's build system, not project tooling.)

## ABI-sensitive spots

- `Modules/_ctypes/libffi` (or system libffi): `src/x86/unix64.S` and
  `ffi64.c` implement the SysV register ABI with `movsd`/`movaps` on XMM.
  Patch to the knc64-x87 ABI: float/double arguments copied to the stack,
  return read from `st0`. The 2013 k1om patch for zmm is the shape to
  follow; this one is simpler.
- `Modules/_decimal/libmpdec`: C only, fine.
- `Objects/longobject.c` uses `__int128` where available; integer only.
- `Python/pymath.c`, `mathmodule.c`: plain C, x87 via the compiler.
- `_hashlib`/`_ssl`: OpenSSL with `no-asm`.

## Smoke

`python3 -m test test_math test_float test_ctypes test_json test_threading`
on the card, and a `multiprocessing.Pool(228)` map to prove all threads
are usable from Python.

## Build (2026-09-14)

`cpython.sh` cross-builds CPython 3.14.7 (the version must equal the
host's `python3`, which runs the build's own scripts through
`--with-build-python`) as a single static interpreter: configure is given `MODULE_BUILDTYPE=static`
(the variable behind the `*shared*` marker of `Modules/Setup.stdlib`) so every module configure
detects (zlib, curses on the sysroot's ncursesw, sockets, json, hashes,
decimal, expat, and the rest) is linked in, because static musl has no
`dlopen`. `PHI_PYTHON_SETUP` appends a `Setup` fragment for extra built-in
modules and `PHI_PYTHON_EXTRA_SRC` copies their sources into `Modules/`;
`glances-phi` uses this for psutil. The interpreter is audited and run on
the host before packaging; the package installs under `/opt/phi`
(`bin/python3.14`, `lib/python3.14`), tests, idle, tkinter and turtledemo
removed. `_hmac` is left out (its HACL* sources duplicate the hash
modules' in a static link; `hmac` falls back to Python). No ctypes (libffi's x86-64 code assumes SSE for floating-point
arguments; a knc64-x87 port is future work), no ssl (no OpenSSL for the
card yet), no sqlite3, no bz2/lzma.

Cross-build gotcha: configure asks pkg-config about lzma, bzip2, zstd,
openssl and sqlite; left alone it finds the host's copies and the build
then fails compiling `_lzma`, `_bz2` and `_zstd` against headers the
sysroot does not have. The script sets `PKG_CONFIG_LIBDIR` to the sysroot's
(empty) pkgconfig directory; zlib and curses are found by header and
library probes.

`PHI_PYTHON_INCREMENTAL=1` keeps a configured tree and only refreshes the
extra module sources, rewrites `Setup.local` and reruns `make`: seconds
instead of minutes when iterating on a fragment.

## The `knc` module (2026-09-20)

The interpreter on the card is now built with the vector unit in it:

```sh
card/lib/knc/build.sh                       # libknc.a into the sysroot first
PHI_PYTHON_EXTRA_SRC=card/lib/knc-py \
PHI_PYTHON_SETUP=card/lib/knc-py/Setup.local \
  card/userland/components/cpython.sh
```

`import knc` then gives bit packing at every width from 1 to 32 and a block
copy, running on hand-encoded MVEX. `card/lib/knc-py/kncmodule.md` covers
it; the audit of the interpreter reports 19848 KNC vector instructions,
which is `libknc` having been linked in.

The absent `_ctypes` is the reason this is a built-in rather than an
extension, and the reason is structural rather than a missing dependency:
static musl has no `dlopen`, so there is nothing for `ctypes.CDLL` to load
even with libffi ported. A built-in module is the only route from Python to
compiled code here.

HACL*'s Blake2 SIMD variants: configure compile-tests `-msse4.1` and
`-mavx2`, builds `Hacl_Hash_Blake2*_Simd*.c` with those flags (per-file
flags win over the wrapper's `-mno-sse`) and dispatches on CPUID. The
card would never execute them, but they fail the audit (1918
instructions on the first static link), so the script answers both
compile tests with `no` through their cache variables.

mimalloc: CPython's bundled allocator spins with an inline-assembly
`pause` (14 sites), which the card lacks and the compiler-level fix
cannot reach. It is only required for the free-threaded build, so the
script configures `--without-mimalloc` (pymalloc is used).
