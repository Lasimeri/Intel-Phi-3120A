# vpu-layout-check.c

Prints every offset of the offload protocol as the C compiler lays it out
(`card/vpu/vpu_proto.h`) against the numbers the Rust side's unit tests
assert (`host/crates/phi-vpu/src/proto.rs`), and exits non-zero on any
mismatch.

```
tcc -run tools/vpu-layout-check.c      # or: make layout-check
```

The two sides share memory with nothing checking the layout at run time,
so this is the check. It is a sibling of `ring-layout-check.c`, which does
the same for the ring transport.
