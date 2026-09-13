# musl

Upstream musl (1.2.x), built with `knc-cc`. `docs/decisions/0006`.

## ABI-sensitive spots

- `src/math/x86_64/*.s`: `fabs`, `sqrt`, `fma`, `remquo`, `rint`, `lrint`
  and friends use SSE. Build with these excluded so the C generics are used
  (`ARCH=x86_64` with the `.s` files removed from the tree, or a patch that
  renames the directory).
- `src/string/x86_64/{memcpy,memmove,memset}.s`: use `rep movsq`/`stosq`,
  which KNC supports; keep, audit confirms.
- `src/setjmp/x86_64/*.s`, `src/thread/x86_64/*.s`, `crt/x86_64/*.s`,
  `src/ldso/x86_64/*.s`: integer only; keep.
- `src/internal/x86_64/`: `syscall` instruction, supported.
- Float classification follows the compiler: with the patched LLVM,
  `double` arguments arrive on the stack and return in `st0`, and musl's C
  code neither knows nor cares.

## Build

```
CC=knc-cc ./configure --target=x86_64 --prefix=/ --syslibdir=/lib --disable-shared
make -j && make DESTDIR=$SYSROOT install
```

Then the sysroot is what `knc-cc --sysroot` points at.

## Smoke

`hello` (printf), `pthread` (create/join 228 threads), `math` (sqrt, sin,
pow, printed with `%g`), each audited and run on the card.
