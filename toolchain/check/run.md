# toolchain/check/run.sh

Phase P2's exit criterion as a script. Three verdicts, all mechanical:

1. **Audit clean.** `phi-isa-audit` without `--allow-suspect`: zero illegal
   and zero suspect instructions, so multi-byte NOPs and `endbr64` must be
   absent too (patch 0003, `-fcf-protection=none`).
2. **Runs on the host.** The binary is a static x86-64 ELF using only
   instructions the host also has, so the host kernel runs it. Exit status
   0 means every computed value matched (`hello.md`).
3. **Visible x87 return.** The disassembly of `scale` ends in `fstp`/`ret`
   with the result on the x87 stack, not `movsd %xmm0`.

When `toolchain/rust/build-std.sh` has produced `libhello_rs.a`, the script
links it in (with `-DHAVE_RUST` so `hello.c` uses the real symbol, and
`-lunwind` from `toolchain/libunwind/build.sh` because Rust's `std` on static
musl references the unwinder for backtraces). The audit then covers the Rust
code paths that survived `--gc-sections`, and the exit status includes the
`rust_hypot(3, 4) == 5` check.
