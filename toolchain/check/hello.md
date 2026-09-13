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
| `rust_hypot` (weak) | when the Rust half is linked, C calls Rust with two doubles and gets a double back across the shared ABI |

The exit status is the verdict: 0 means every value matched. `run.sh`
compiles it, audits it, and runs it on the host.
