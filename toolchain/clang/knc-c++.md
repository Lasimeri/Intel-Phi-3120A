# knc-c++

Thin C++ front for `knc-cc`: adds `-x c++ -stdlib=libc++`. Usable for
compiling (`-c`) as soon as the patched clang exists; linking C++ programs
needs libc++ built for the card (phase P7, `card/userland/components/clang.md`).
