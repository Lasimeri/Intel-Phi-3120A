# kncmodule.c (the Python `knc` module)

The card's vector unit from Python, as a module built **into** the
interpreter.

```python
import knc

values = knc.Buffer(knc.BLOCK * 4)
packed = knc.Buffer(knc.packed_bytes(11))
memoryview(values).cast("I")[:] = range(knc.BLOCK)
knc.pack(packed, values, 11)
knc.unpack(values, packed, 11)
```

## Built in, not loaded

The card's CPython is static against musl, which has no `dlopen`. There are
no `.so` extension modules to import, and `_ctypes` is missing for the same
reason: even with libffi present it could never load a shared library here.
So a built-in module is not a workaround, it is the only route from Python
to compiled code on this machine.

`card/userland/components/cpython.sh` has the hooks:

```sh
PHI_PYTHON_EXTRA_SRC=card/lib/knc-py \
PHI_PYTHON_SETUP=card/lib/knc-py/Setup.local \
  card/userland/components/cpython.sh
```

`Setup.local` is one line, `knc knc-py/kncmodule.c -lknc`, and `libknc.a`
comes from the sysroot, so `card/lib/knc/build.sh` has to have run first.

## `knc.Buffer`

CPython's allocator gives 16-byte alignment. Every kernel loads and stores
whole 64-byte vectors and has no unaligned form, so a `bytes` or a
`bytearray` is not a valid argument and `knc.Buffer(nbytes)` is how a caller
gets one that is: 64-byte aligned, zeroed, and exporting the buffer
protocol, so `memoryview(buf)` and `buf[:] = data` work normally.

It reports `readonly = 0` whatever the caller asked for, the way `bytearray`
does. Deriving that from `PyBUF_WRITABLE` instead, which is the obvious
reading of the API, makes a plain `memoryview(buf)` read-only and every
assignment through it fail.

## Whole buffers per call

`unpack(out, packed, bits)` and `pack(packed, values, bits)` walk every
whole block both buffers hold and return the count, with the GIL released.
That is deliberate: a per-block Python call would cost more than the kernel
it wraps. Decoding 64 blocks in one call reaches 395.7 M values per second,
against 403.9 for the same work from C, so the interpreter is costing about
2 percent.

## Errors

A width outside 1 to 32 raises `ValueError`, where the C entry point returns
silently (the dispatch is a computed jump, and a library called from Python
cannot assume the width came from a programmer). A buffer that is not
64-byte aligned raises `ValueError` naming `knc.Buffer`.

## Measured, 2026-09-20, one call, one thread

| bits | unpack M/s |
| --- | --- |
| 1 | 563.0 |
| 8 | 471.7 |
| 11 | 395.7 |
| 16 | 325.2 |
| 32 | 195.6 |

`knc_demo.py` produces these and checks all 32 widths against a scalar
model of the layout written in Python.
