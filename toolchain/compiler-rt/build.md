# toolchain/compiler-rt/build.sh

The compiler runtime (`libclang_rt.builtins`) for the card: 128-bit
integer division, float conversions the backend expands to calls, and the
rest of the helpers clang assumes exist. Built with `knc-cc` against the
musl sysroot and installed into the patched clang's resource directory so
no extra `-L` is ever needed.

Sanitizers, profiling, XRay, fuzzing runtimes are off: none of them run
without SSE and none are wanted on the card.

Must run after `toolchain/llvm/build.sh` and `toolchain/musl/build.sh`.
Ends with an audit of the archive.
