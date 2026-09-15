#!/usr/bin/env bash
# build.sh: build libc++abi and libc++ for the card (static, on musl, with
# compiler-rt and the LLVM unwinder built earlier) and install them into
# the card sysroot. Needed by any C++ program for the card, first of all
# the native clang (phase P7). Mirrors toolchain/libunwind/build.sh; see
# build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../env.sh"
root="$phi_root"
SRC="${PHI_LLVM_SRC:-$root/toolchain/build/llvm-project}"
PREFIX="$PHI_LLVM"
SYSROOT="$PHI_SYSROOT"
BUILD="$root/toolchain/build/libcxx-build"
CC=knc-cc
CXX=knc-c++
rm -rf "$BUILD"
cmake -S "$SRC/runtimes" -B "$BUILD" -G Ninja \
    -DLLVM_ENABLE_RUNTIMES="libunwind;libcxxabi;libcxx" \
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
    -DLIBUNWIND_INCLUDE_DOCS=OFF \
    -DLIBCXXABI_ENABLE_SHARED=OFF \
    -DLIBCXXABI_ENABLE_STATIC=ON \
    -DLIBCXXABI_USE_COMPILER_RT=ON \
    -DLIBCXXABI_USE_LLVM_UNWINDER=ON \
    -DLIBCXXABI_ENABLE_STATIC_UNWINDER=ON \
    -DLIBCXXABI_INCLUDE_TESTS=OFF \
    -DLIBCXXABI_INSTALL_LIBRARY_DIR=lib \
    -DLIBCXX_ENABLE_SHARED=OFF \
    -DLIBCXX_ENABLE_STATIC=ON \
    -DLIBCXX_USE_COMPILER_RT=ON \
    -DLIBCXX_HAS_MUSL_LIBC=ON \
    -DLIBCXX_CXX_ABI=libcxxabi \
    -DLIBCXX_ENABLE_STATIC_ABI_LIBRARY=ON \
    -DLIBCXX_STATICALLY_LINK_ABI_IN_STATIC_LIBRARY=ON \
    -DLIBCXX_INCLUDE_TESTS=OFF \
    -DLIBCXX_INCLUDE_BENCHMARKS=OFF \
    -DLIBCXX_INCLUDE_DOCS=OFF \
    -DLIBCXX_INSTALL_LIBRARY_DIR=lib \
    -DLIBCXX_INSTALL_INCLUDE_DIR=include/c++/v1 \
    -DLIBCXX_INSTALL_INCLUDE_TARGET_DIR=include/c++/v1
cmake --build "$BUILD" -j"$(nproc)"
cmake --install "$BUILD"
ls -l "$SYSROOT/usr/lib/libc++.a" "$SYSROOT/usr/lib/libc++abi.a" "$SYSROOT/usr/include/c++/v1/vector"
for lib in libc++abi libc++; do
    echo "== audit $SYSROOT/usr/lib/$lib.a"
    "$root/host/target/debug/phi-isa-audit" "$SYSROOT/usr/lib/$lib.a"
done
