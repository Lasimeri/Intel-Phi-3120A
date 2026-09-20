# qjs_knc.c and qjs_knc.h (the QuickJS `knc` module)

The card's vector unit from JavaScript, as a native module compiled into
`qjs`.

```js
import * as knc from "knc";

const values = knc.alloc(knc.BLOCK * 4);
const packed = knc.alloc(knc.packedBytes(11));
new Uint32Array(values).set(Array.from({length: knc.BLOCK}, (_, i) => i));
knc.pack(packed, values, 11);
knc.unpack(values, packed, 11);
```

## Compiled in, not loaded

QuickJS can load native modules as shared objects. The card cannot: its
binaries are static against musl and there is no `dlopen`. So
`card/userland/components/quickjs.sh` compiles this file into `qjs` and
inserts one line into `JS_NewCustomContext`, next to where the shell already
registers `std` and `os`:

```c
js_init_module_knc(ctx, "knc");
```

`import * as knc from "knc"` then works from any script with no path, no
loader and no build step.

## `knc.alloc`

QuickJS's ArrayBuffers are whatever `malloc` returned, and the kernels need
64-byte alignment with no unaligned fallback. `knc.alloc(nbytes)` returns a
real `ArrayBuffer` over memory this file aligned, so `new Uint32Array(buf)`,
`slice`, and everything else behave normally, and the buffer is freed by the
runtime when it is collected.

Passing a plain `new ArrayBuffer(...)` raises `TypeError` naming
`knc.alloc`, unless `malloc` happened to return a 64-byte aligned block, in
which case it works and the demo says so rather than pretending otherwise.

Both `ArrayBuffer` and any view over one are accepted, because a caller will
have a `Uint32Array` in hand after filling the values and refusing it would
force a slice at every call site.

## Whole buffers per call

As in the Python module, one call walks every whole block both buffers hold
and returns the count. 398.9 M values per second at 11 bits, against 403.9
from C: the interpreter is costing about 1 percent, because it is called
once for 64 blocks.

## The PAUSE patch

The build patches one line of `quickjs.c`. `Atomics.pause()` reaches for the
PAUSE instruction through inline assembly guarded on `__x86_64__`, and KNC
deletes PAUSE (ISA reference 327364-001, appendix B.2). Requiring `__SSE2__`
as well sends this card to the empty no-op the file already has for other
architectures and changes nothing anywhere else.

That is the third dialect of the same portability bug in this project: xz
guarded CMOV assembly on `__x86_64__` (`xz.md`), zstd used function-level
target attributes with CPUID dispatch (`zstd.md`), and this is inline
assembly again. All three assume x86-64 implies a feature set, and none of
them is visible in a build log.

## Measured, 2026-09-20, one call, one thread

| bits | unpack M/s |
| --- | --- |
| 1 | 581.5 |
| 8 | 477.3 |
| 11 | 398.9 |
| 16 | 401.7 |
| 32 | 231.3 |

`knc_demo.js` produces these and checks all 32 widths against a scalar model
of the layout written in JavaScript (in unsigned arithmetic throughout,
because at 32 bits a value fills the word and JavaScript's bitwise operators
are signed).
