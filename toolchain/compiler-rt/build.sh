#!/usr/bin/env bash
# build.sh: compiler-rt builtins for the card target, built with knc-cc
# against the musl sysroot, installed into the patched clang's resource
# directory so `knc-cc` finds libclang_rt.builtins automatically. See build.md.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../env.sh"
root="$phi_root"
SRC="${PHI_LLVM_SRC:-$root/toolchain/build/llvm-project}"
PREFIX="$PHI_LLVM"
SYSROOT="$PHI_SYSROOT"
BUILD="$root/toolchain/build/compiler-rt-build"
CC=knc-cc
resdir=$("$PREFIX/bin/clang" -print-resource-dir)
# Always a clean build: it takes two minutes, and ninja would otherwise keep
# objects compiled by an older clang when only the compiler changed.
rm -rf "$BUILD"

cmake -S "$SRC/compiler-rt" -B "$BUILD" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_COMPILER="$CC" \
    -DCMAKE_ASM_COMPILER="$CC" \
    -DCMAKE_C_COMPILER_TARGET=x86_64-unknown-linux-musl \
    -DCMAKE_ASM_COMPILER_TARGET=x86_64-unknown-linux-musl \
    -DCMAKE_SYSROOT="$SYSROOT" \
    -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY \
    -DCMAKE_AR="$PREFIX/bin/llvm-ar" \
    -DCMAKE_RANLIB="$PREFIX/bin/llvm-ranlib" \
    -DCMAKE_NM="$PREFIX/bin/llvm-nm" \
    -DLLVM_CMAKE_DIR="$PREFIX/lib/cmake/llvm" \
    -DCOMPILER_RT_DEFAULT_TARGET_ONLY=ON \
    -DCOMPILER_RT_BUILD_BUILTINS=ON \
    -DCOMPILER_RT_BUILD_SANITIZERS=OFF \
    -DCOMPILER_RT_BUILD_XRAY=OFF \
    -DCOMPILER_RT_BUILD_LIBFUZZER=OFF \
    -DCOMPILER_RT_BUILD_PROFILE=OFF \
    -DCOMPILER_RT_BUILD_MEMPROF=OFF \
    -DCOMPILER_RT_BUILD_ORC=OFF \
    -DCOMPILER_RT_BUILD_GWP_ASAN=OFF \
    -DCOMPILER_RT_BUILD_CTX_PROFILE=OFF \
    -DCOMPILER_RT_INSTALL_PATH="$resdir" \
    -DCOMPILER_RT_BAREMETAL_BUILD=OFF \
    -DCOMPILER_RT_X86_NO_SSE=ON
cmake --build "$BUILD" -j"$(nproc)"
cmake --install "$BUILD"
lib=$(find "$resdir/lib" -name 'libclang_rt.builtins*.a' | head -1)
echo "== audit $lib"
"$root/host/target/debug/phi-isa-audit" --allow-suspect "$lib"
