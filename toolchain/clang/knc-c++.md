# knc-c++

Thin C++ front for `knc-cc`: adds `-x c++ -stdlib=libc++`. Usable for
compiling (`-c`) as soon as the patched clang exists; linking C++ programs
needs libc++ built for the card (phase P7, `card/userland/components/clang.md`).

2026-09-14: the wrapper now selects C++ with `--driver-mode=g++` instead of
`-x c++`. The latter also applied to object files given at link time, so
any C++ link step through the wrapper failed (CMake's compiler test for the
card LLVM build was the first to hit it; the runtimes builds never link
executables). `-stdlib=libc++` stays.
