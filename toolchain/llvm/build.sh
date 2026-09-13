configure() {
    local gen="Ninja"
    command -v ninja >/dev/null || gen="Unix Makefiles"
    echo "== configuring ($gen, variant $VARIANT) into $BUILD, install to $PREFIX"
    if [ "$VARIANT" = dylib ]; then
        cmake -S "$SRC/llvm" -B "$BUILD" -G "$gen" \
            -DCMAKE_BUILD_TYPE=Release \
            -DCMAKE_INSTALL_PREFIX="$PREFIX" \
            -DLLVM_ENABLE_PROJECTS="" \
            -DLLVM_TARGETS_TO_BUILD=all \
            -DLLVM_BUILD_LLVM_DYLIB=ON \
            -DLLVM_LINK_LLVM_DYLIB=ON \
            -DLLVM_ENABLE_RTTI=ON \
            -DLLVM_ENABLE_FFI=ON \
            -DLLVM_ENABLE_ASSERTIONS=OFF \
            -DLLVM_INCLUDE_TESTS=OFF \
            -DLLVM_INCLUDE_BENCHMARKS=OFF \
            -DLLVM_INCLUDE_EXAMPLES=OFF \
            -DLLVM_INCLUDE_DOCS=OFF \
            -DLLVM_ENABLE_BINDINGS=OFF \
            -DLLVM_PARALLEL_LINK_JOBS=2 \
            -DCMAKE_C_COMPILER=clang \
            -DCMAKE_CXX_COMPILER=clang++ \
            -DLLVM_USE_LINKER=lld
        return
    fi
    cmake -S "$SRC/llvm" -B "$BUILD" -G "$gen" \#!/usr/bin/env bash
# build.sh: fetch, patch, build, and install the project's LLVM (clang + lld)
# into toolchain/build/llvm. Reproducible: pinned tag, patches from
# toolchain/llvm/patches/ applied in SERIES order, fixed cmake options.
# See build.md. Usage: toolchain/llvm/build.sh [configure|build|install|all|check]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
TAG="${PHI_LLVM_TAG:-llvmorg-22.1.8}"
SRC="${PHI_LLVM_SRC:-$root/toolchain/build/llvm-project}"
# VARIANT=clang (default): static X86-only clang+lld, the card C compiler.
# VARIANT=dylib: libLLVM.so mirroring Arch's llvm-libs options (all targets,
# RTTI, FFI) so the distro rustc can load the patched backend via
# LD_LIBRARY_PATH; no clang, no lld. See build.md.
VARIANT="${PHI_LLVM_VARIANT:-clang}"
if [ "$VARIANT" = dylib ]; then
    BUILD="${PHI_LLVM_BUILD:-$root/toolchain/build/llvm-build-dylib}"
    PREFIX="${PHI_LLVM:-$root/toolchain/build/llvm-dylib}"
else
    BUILD="${PHI_LLVM_BUILD:-$root/toolchain/build/llvm-build}"
    PREFIX="${PHI_LLVM:-$root/toolchain/build/llvm}"
fi
JOBS="${PHI_JOBS:-$(nproc)}"
step="${1:-all}"

fetch() {
    if [ ! -d "$SRC/.git" ]; then
        echo "== fetching llvm-project $TAG"
        git clone --depth 1 --branch "$TAG" https://github.com/llvm/llvm-project.git "$SRC"
    else
        echo "== have llvm-project at $(git -C "$SRC" describe --tags --always)"
    fi
}

patch_tree() {
    local series="$here/patches/SERIES"
    [ -f "$series" ] || { echo "== no patches (SERIES missing)"; return; }
    # Start from the pristine tag every time so the result does not depend
    # on what was applied before.
    git -C "$SRC" checkout -q -- . && git -C "$SRC" clean -qfd
    while IFS= read -r p; do
        [ -z "$p" ] || [ "${p#\#}" != "$p" ] && continue
        echo "== applying $p"
        git -C "$SRC" apply --index "$here/patches/$p"
    done < "$series"
}

configure() {
    local gen="Ninja"
    command -v ninja >/dev/null || gen="Unix Makefiles"
    echo "== configuring ($gen) into $BUILD, install to $PREFIX"
    cmake -S "$SRC/llvm" -B "$BUILD" -G "$gen" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$PREFIX" \
        -DLLVM_ENABLE_PROJECTS="clang;lld" \
        -DLLVM_TARGETS_TO_BUILD=X86 \
        -DLLVM_DEFAULT_TARGET_TRIPLE=x86_64-unknown-linux-musl \
        -DLLVM_ENABLE_ASSERTIONS=OFF \
        -DLLVM_INCLUDE_TESTS=OFF \
        -DLLVM_INCLUDE_BENCHMARKS=OFF \
        -DLLVM_INCLUDE_EXAMPLES=OFF \
        -DLLVM_INCLUDE_DOCS=OFF \
        -DLLVM_ENABLE_BINDINGS=OFF \
        -DLLVM_ENABLE_ZLIB=ON \
        -DLLVM_ENABLE_ZSTD=OFF \
        -DLLVM_ENABLE_LIBXML2=OFF \
        -DLLVM_ENABLE_TERMINFO=OFF \
        -DLLVM_PARALLEL_LINK_JOBS=4 \
        -DCLANG_DEFAULT_LINKER=lld \
        -DCLANG_DEFAULT_RTLIB=compiler-rt \
        -DCLANG_DEFAULT_CXX_STDLIB=libc++ \
        -DCLANG_DEFAULT_UNWINDLIB=none \
        -DCMAKE_C_COMPILER=clang \
        -DCMAKE_CXX_COMPILER=clang++ \
        -DLLVM_USE_LINKER=lld
}

build() {
    echo "== building with $JOBS jobs"
    if [ "$VARIANT" = dylib ]; then
        cmake --build "$BUILD" -j "$JOBS" --target LLVM
    else
        cmake --build "$BUILD" -j "$JOBS"
    fi
}

install() {
    echo "== installing to $PREFIX"
    if [ "$VARIANT" = dylib ]; then
        # Only the shared library is needed; cmake --install would want every tool built.
        mkdir -p "$PREFIX/lib"
        cp -a "$BUILD"/lib/libLLVM*.so* "$PREFIX/lib/"
        ls -l "$PREFIX/lib/" | grep libLLVM
        return
    fi
    cmake --install "$BUILD"
    "$PREFIX/bin/clang" --version | head -1
}| head -1
}

# check: compile the two probe files from docs/research/abi-and-toolchain.md
# with the wrapper and audit the result. Fails until the patches are in.
check() {
    local tmp; tmp=$(mktemp -d)
    printf 'int f(int a,int b,int c){return c?a:b;}\nlong g(long a,long b,int c){return c?a:b;}\n' > "$tmp/t.c"
    printf 'double m(double a,double b){return a*b+1.5;}\nfloat h(float a){return a/3.0f;}\n' > "$tmp/d.c"
    PHI_LLVM="$PREFIX" "$root/toolchain/clang/knc-cc" -O2 -c "$tmp/t.c" -o "$tmp/t.o" -nostdinc -nostdlib
    PHI_LLVM="$PREFIX" "$root/toolchain/clang/knc-cc" -O2 -c "$tmp/d.c" -o "$tmp/d.o" -nostdinc -nostdlib
    "$root/host/target/debug/phi-isa-audit" --allow-suspect "$tmp/t.o"
    "$root/host/target/debug/phi-isa-audit" --allow-suspect "$tmp/d.o"
    "$PREFIX/bin/llvm-objdump" -d "$tmp/d.o" | grep -E "fld|fmul|fstp|ret" | head -8
    rm -rf "$tmp"
}

case "$step" in
    fetch) fetch ;;
    patch) fetch; patch_tree ;;
    configure) fetch; patch_tree; configure ;;
    build) build ;;
    install) install ;;
    check) check ;;
    all) fetch; patch_tree; configure; build; install; check ;;
    *) echo "usage: build.sh [fetch|patch|configure|build|install|check|all]" >&2; exit 2 ;;
esac
