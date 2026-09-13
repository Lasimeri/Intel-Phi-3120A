# tools/

Small C helpers compiled and run with `tcc`, used where a check has to see
a C header exactly as C sees it. They are not part of any build product.

| Tool | Purpose |
| --- | --- |
| `ring-layout-check.c` | Prints `sizeof`/`offsetof` for the ring transport structures in `card/drivers/phinet/include/phi_ring.h` and compares them with the values the Rust crate uses. `make layout-check`. |
