#!/usr/bin/env bash
# build.sh: LLVM libunwind for the card target, built with knc-cc/knc-c++
# against the musl sysroot and installed into it as a static library.
# Needed by Rust's std on static musl (the `unwind` crate links
# libunwind.a) and later by C++ on the card. See build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../env.sh"
root="$phi_root"
SRC="${PHI_LLVM_SRC:-$root/toolchain/build/llvm-project}"
PREFIX="$PHI_LLVM"
SYSROOT="$PHI_SYSROOT"
BUILD="$root/toolchain/build/libunwind-build"
CC=knc-cc
CXX=knc-c++
# Always a clean build (same reasoning as compiler-rt: ninja keeps objects
# from an older compiler when only the compiler changed; the build is short).
rm -rf "$BUILD"

cmake -S "$SRC/runtimes" -B "$BUILD" -G Ninja \
    -DLLVM_ENABLE_RUNTIMES=libunwind \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_COMPILER="$CC" \
    -DCMAKE_CXX_COMPILER="$CXX" \
    -DCMAKE_ASM_COMPILER="$CC" \
    -DCMAKE_C_COMPILER_TARGET=x86_64-unknown-linux-musl \
    -DCMAKE_CXX_COMPILER_TARGET=x86_64-unknown-linux-musl \
    -DCMAKE_ASM_COMPILER_TARGET=x86_64-unknown-linux-musl \
    -DCMAKE_SYSROOT="$SYSROOT" \
    -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY \
    -DCMAKE_AR="$PREFIX/bin/llvm-ar" \
    -DCMAKE_RANLIB="$PREFIX/bin/llvm-ranlib" \
    -DCMAKE_NM="$PREFIX/bin/llvm-nm" \
    -DCMAKE_INSTALL_PREFIX="$SYSROOT/usr" \
    -DLIBUNWIND_ENABLE_SHARED=OFF \
    -DLIBUNWIND_ENABLE_STATIC=ON \
    -DLIBUNWIND_USE_COMPILER_RT=ON \
    -DLIBUNWIND_HERMETIC_STATIC_LIBRARY=ON \
    -DLIBUNWIND_ENABLE_CROSS_UNWINDING=OFF \
    -DLIBUNWIND_INSTALL_LIBRARY_DIR=lib \
    -DLIBUNWIND_INSTALL_INCLUDE_DIR=include \
    -DLIBUNWIND_INCLUDE_TESTS=OFF \
    -DLIBUNWIND_INCLUDE_DOCS=OFF
cmake --build "$BUILD" -j"$(nproc)"
cmake --install "$BUILD"
lib="$SYSROOT/usr/lib/libunwind.a"
ls -l "$lib" "$SYSROOT/usr/include/unwind.h"
echo "== audit $lib"
"$root/host/target/debug/phi-isa-audit" "$lib"
