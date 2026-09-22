# build.sh: build the worker on the card

```
sh build.sh
```

Compiles `vpu_worker.c` with the translated kernel
`card/examples/avx512_poly.S` into `phi-vpu-worker`, using the card's own
clang (`cc`). It runs on the card, not the host: the host's compiler does
not target Knights Corner.

`scripts/phi-vpu.sh deploy` copies this directory's sources and the
kernel to `/opt/phi-vpu` on the card and runs this script there, which is
why it accepts a copy of `avx512_poly.S` next to itself as well as the
one in `../examples`. There is one kernel source, in `card/examples`; a
second copy in this directory would drift.
