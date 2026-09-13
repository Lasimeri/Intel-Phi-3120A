# ring-layout-check.c

Compiles `phi_ring.h` with `tcc` and prints every structure size and field
offset next to the value hard-coded in `host/crates/phi-ring/src/layout.rs`.
The Rust side has the same numbers under test; this tool closes the loop
from the C side, since a C compiler's idea of padding is the one the kernel
module will use.

```
$ tcc -run tools/ring-layout-check.c
ok       sizeof(phi_region_hdr)       64
...
```

Non-zero exit on any mismatch. Wired into `make layout-check` and `make check`.
