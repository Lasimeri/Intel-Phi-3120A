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
