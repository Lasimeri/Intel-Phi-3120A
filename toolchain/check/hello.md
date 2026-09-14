# toolchain/check/hello.c

The C half of the phase P2 exit test. Every construct in it maps to one
of the ABI amendments or one of the deletion-list risks:

| Construct | What it proves |
| --- | --- |
| `scale`, `halve` | double/float arguments on the stack, returns in `ST0` (patch 0002) |
| `sum_va` | `va_arg(ap, double)` reads the overflow area (patch 0004) |
| `bigsum` | `long double` unchanged (x87 in both ABIs) |
| `pick` | a 64-bit select compiled without CMOV (patches 0001, 0005) |
| `sqrt`, `printf("%g")`, `malloc`, `pthread_create` | musl's math, stdio, allocator, and threads built for the card |
| `rust_hypot` | with `-DHAVE_RUST` (set by `run.sh` when `libhello_rs.a` exists) C calls Rust with two doubles and checks the double coming back across the shared ABI; without it a weak stub returns -1. The real declaration must not be weak: a weak definition in `hello.o` would satisfy the linker and the archive member would never be pulled in |

The exit status is the verdict: 0 means every value matched (1 arithmetic or
threads, 2 `sqrt`, 3 the Rust result). `run.sh`
compiles it, audits it, and runs it on the host.
