# phi-isa-audit / main.rs

Command-line front end for the audit engine.

```
phi-isa-audit vmlinux                 # summary: one line per reason
phi-isa-audit --list --max 50 a.out   # every hit with address and symbol
phi-isa-audit --allow-suspect libc.so # do not fail on multi-byte NOPs
phi-isa-audit --ignore XSAVE libstd.a  # report XSAVE hits, do not count them
```

`--ignore REASON` (repeatable, case-insensitive, matched against the
reason column) is for documented false positives in archives that bundle
code no program links, such as `xgetbv` in Rust's `std_detect`. Ignored
hits are still printed, prefixed `IGNORED`, and the result line gains an
`ignored` count. Linked executables are audited without it.

Exit status is the contract for build scripts: 0 clean, 1 hits, 2 error.
`make audit BIN=path` wraps it.

## Where it is used

- Phase P2 exit criterion: a static `hello` in C and Rust from the card
  toolchain must report `0 illegal`.
- Phase P3: `vmlinux` of the card kernel, plus every `.ko`.
- Phase P4 onward: every binary that goes into the initramfs, run from the
  initramfs build script.

## Reading the output

A hit inside a function that is provably never executed on KNC (an
`alternative()` slot the kernel patches out, an IFUNC variant selected only
when CPUID advertises the feature) is a false positive. The kernel keeps
such code in `.altinstr_replacement`, which the audit skips by name (it
is an executable section, but its bytes are only copied into the text
when the keyed CPU feature is present); IFUNC variants in musl do not
exist. When a hit is a genuine false
positive, record it in the build script that invokes the audit, with the
reason.
