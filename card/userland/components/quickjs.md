# QuickJS

`docs/decisions/0004`. Upstream QuickJS (or QuickJS-ng), `CONFIG_LTO`
off, built with `knc-cc` against musl. Pure C99; doubles go through the C
ABI; no assembly; `libm` from musl. `qjs` and `qjsc` both ship.

Smoke: the `tests/` directory from upstream, plus a
`setTimeout`/`Promise` microbenchmark to record single-thread performance
of a 1.1 GHz in-order core, which will be humbling.
