# gcc on the card

Requested explicitly; it needs the same two ABI amendments the LLVM patch
makes, in `gcc/config/i386/i386.cc`:

1. `function_value_64`: for `SFmode`/`DFmode` when `!TARGET_SSE`, return in
   `FIRST_FLOAT_REG` instead of emitting "SSE register return with SSE
   disabled".
2. `classify_argument`/`construct_container`: classify `SFmode`/`DFmode` as
   `X86_64_MEMORY_CLASS` when `!TARGET_SSE` instead of erroring.
3. A `-mno-cmov` style switch for 64-bit mode: `TARGET_CMOV` is derived from
   `ix86_arch_features`; add an option that forces it off so `-march=x86-64`
   can be used without CMOV. Verify with `phi-isa-audit`.

Then: build gcc as a cross (host to card) with musl as the target libc,
verify its output with the audit tool and by running on the card, then
build it again with itself to get a native gcc (three-stage bootstrap,
standard).

This is phase P8 and is independent of clang, which comes first because
it needs no backend changes beyond the shared LLVM patch.
