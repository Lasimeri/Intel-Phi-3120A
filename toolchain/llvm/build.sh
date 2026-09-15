#!/usr/bin/env bash
# build.sh: fetch, patch, build, and install the project's LLVM.
# Reproducible: pinned tag, patches from toolchain/llvm/patches/ applied in
# SERIES order onto a hard-reset tree, fixed cmake options. See build.md.
#
# Usage: toolchain/llvm/build.sh [fetch|patch|configure|build|install|check|all]
#
# PHI_LLVM_VARIANT=clang (default): static X86-only clang+lld, the card C
#   compiler, installed to toolchain/build/llvm.
# PHI_LLVM_VARIANT=dylib: libLLVM.so mirroring the options of Arch's llvm-libs
# PHI_LLVM_VARIANT=card: clang, lld and the binutils-style tools built FOR
#   the card with knc-cc/knc-c++ (a Canadian cross), static on musl and
#   libc++, X86 only, with the host build's tablegen; not installed, the
#   package script copies the binaries (card/userland/components/clang.sh).
#   package (all targets, RTTI, FFI) so the distro rustc can load the patched
#   backend through LD_LIBRARY_PATH; no clang, no lld; installed to
#   toolchain/build/llvm-dylib.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../env.sh"
# env.sh prepares the CARD toolchain environment: CC=knc-cc, CXX=knc-c++ and
# the patched clang first on PATH. This script builds LLVM for the HOST, so
# those must not leak into cmake: the host compilers are pinned by absolute
# path (override with PHI_HOST_CC / PHI_HOST_CXX) and CC/CXX are unset.
unset CC CXX
HOST_CC="${PHI_HOST_CC:-/usr/bin/clang}"
HOST_CXX="${PHI_HOST_CXX:-/usr/bin/clang++}"
# Real, space-free paths (see env.md): cmake and ninja record absolute paths
# and the LLVM shared-library link passes a version-script path unquoted.
root="$phi_build/.."
TAG="${PHI_LLVM_TAG:-llvmorg-22.1.8}"
SRC="${PHI_LLVM_SRC:-$phi_build/toolchain/llvm-project}"
VARIANT="${PHI_LLVM_VARIANT:-clang}"
if [ "$VARIANT" = dylib ]; then
    BUILD="${PHI_LLVM_BUILD:-$phi_build/toolchain/llvm-build-dylib}"
    PREFIX="${PHI_LLVM_DYLIB:-$phi_build/toolchain/llvm-dylib}"
elif [ "$VARIANT" = card ]; then
    BUILD="${PHI_LLVM_BUILD:-$phi_build/toolchain/llvm-build-card}"
    PREFIX="${PHI_LLVM_CARD:-$phi_build/toolchain/llvm-card}"
    HOST_BUILD="$phi_build/toolchain/llvm-build"
else
    BUILD="${PHI_LLVM_BUILD:-$phi_build/toolchain/llvm-build}"
    PREFIX="$PHI_LLVM"
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
    # Start from the pristine tag every time (index and worktree), so the
    # result never depends on what was applied before.
    git -C "$SRC" reset -q --hard && git -C "$SRC" clean -qfd
    while IFS= read -r p; do
        case "$p" in ''|'#'*) continue ;; esac
        echo "== applying $p"
        git -C "$SRC" apply "$here/patches/$p"
    done < "$series"
    git -C "$SRC" diff --stat | tail -1
}

common_opts() {
    printf '%s\n' \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$PREFIX" \
        -DLLVM_ENABLE_ASSERTIONS=OFF \
        -DLLVM_INCLUDE_TESTS=OFF \
        -DLLVM_INCLUDE_BENCHMARKS=OFF \
        -DLLVM_INCLUDE_EXAMPLES=OFF \
        -DLLVM_INCLUDE_DOCS=OFF \
        -DLLVM_ENABLE_BINDINGS=OFF \
        -DCMAKE_C_COMPILER="$HOST_CC" \
        -DCMAKE_CXX_COMPILER="$HOST_CXX" \
        -DLLVM_USE_LINKER=lld
}

configure() {
    local gen="Ninja"
    command -v ninja >/dev/null 2>&1 || gen="Unix Makefiles"
    echo "== configuring ($gen, variant $VARIANT) into $BUILD, install to $PREFIX"
    local -a opts
    mapfile -t opts < <(common_opts)
    if [ "$VARIANT" = card ]; then
        [ -x "$HOST_BUILD/bin/llvm-tblgen" ] || { echo "card variant needs the host clang variant build in $HOST_BUILD (tablegen)" >&2; exit 1; }
        [ -f "$PHI_SYSROOT/usr/lib/libc++.a" ] || { echo "card variant needs libc++ in the sysroot: toolchain/libcxx/build.sh" >&2; exit 1; }
        cmake -S "$SRC/llvm" -B "$BUILD" -G "$gen" \
            -DCMAKE_BUILD_TYPE=Release \
            -DCMAKE_INSTALL_PREFIX="$PREFIX" \
            -DCMAKE_SYSTEM_NAME=Linux \
            -DCMAKE_SYSTEM_PROCESSOR=x86_64 \
            -DCMAKE_C_COMPILER="$phi_root/toolchain/clang/knc-cc" \
            -DCMAKE_CXX_COMPILER="$phi_root/toolchain/clang/knc-c++" \
            -DCMAKE_ASM_COMPILER="$phi_root/toolchain/clang/knc-cc" \
            -DCMAKE_SYSROOT="$PHI_SYSROOT" \
            -DCMAKE_AR="$PHI_LLVM/bin/llvm-ar" \
            -DCMAKE_RANLIB="$PHI_LLVM/bin/llvm-ranlib" \
            -DCMAKE_NM="$PHI_LLVM/bin/llvm-nm" \
            -DCMAKE_EXE_LINKER_FLAGS="-static" \
            -DLLVM_HOST_TRIPLE=x86_64-unknown-linux-musl \
            -DLLVM_DEFAULT_TARGET_TRIPLE=x86_64-unknown-linux-musl \
            -DLLVM_NATIVE_TOOL_DIR="$HOST_BUILD/bin" \
            -DLLVM_TABLEGEN="$HOST_BUILD/bin/llvm-tblgen" \
            -DCLANG_TABLEGEN="$HOST_BUILD/bin/clang-tblgen" \
            -DLLVM_ENABLE_PROJECTS="clang;lld" \
            -DLLVM_TARGETS_TO_BUILD=X86 \
            -DLLVM_BUILD_STATIC=ON \
            -DLLVM_ENABLE_PIC=OFF \
            -DLLVM_ENABLE_LIBCXX=ON \
            -DLLVM_STATIC_LINK_CXX_STDLIB=ON \
            -DLLVM_ENABLE_THREADS=ON \
            -DLLVM_ENABLE_ASSERTIONS=OFF \
            -DLLVM_ENABLE_ZLIB=OFF \
            -DLLVM_ENABLE_ZSTD=OFF \
            -DLLVM_ENABLE_LIBXML2=OFF \
            -DLLVM_ENABLE_TERMINFO=OFF \
            -DLLVM_ENABLE_LIBEDIT=OFF \
            -DLLVM_ENABLE_LIBPFM=OFF \
            -DLLVM_ENABLE_BINDINGS=OFF \
            -DLLVM_ENABLE_OCAMLDOC=OFF \
            -DLLVM_INCLUDE_TESTS=OFF \
            -DLLVM_INCLUDE_BENCHMARKS=OFF \
            -DLLVM_INCLUDE_EXAMPLES=OFF \
            -DLLVM_INCLUDE_DOCS=OFF \
            -DLLVM_BUILD_UTILS=OFF \
            -DLLVM_DISABLE_ASSEMBLY_FILES=ON \
            -DLLVM_PARALLEL_LINK_JOBS=2 \
            -DCLANG_ENABLE_ARCMT=OFF \
            -DCLANG_ENABLE_STATIC_ANALYZER=OFF \
            -DCLANG_DEFAULT_LINKER=lld \
            -DCLANG_DEFAULT_RTLIB=compiler-rt \
            -DCLANG_DEFAULT_CXX_STDLIB=libc++ \
            -DCLANG_DEFAULT_UNWINDLIB=libunwind
        return
    fi
    mapfile -t opts < <(common_opts)
    if [ "$VARIANT" = dylib ]; then
        cmake -S "$SRC/llvm" -B "$BUILD" -G "$gen" "${opts[@]}" \
            -DLLVM_ENABLE_PROJECTS="" \
            -DLLVM_TARGETS_TO_BUILD=all \
            -DLLVM_BUILD_LLVM_DYLIB=ON \
            -DLLVM_LINK_LLVM_DYLIB=ON \
            -DLLVM_ENABLE_RTTI=ON \
            -DLLVM_ENABLE_FFI=ON \
            -DLLVM_STATIC_LINK_CXX_STDLIB=ON \
            -DLLVM_PARALLEL_LINK_JOBS=2
    else
        cmake -S "$SRC/llvm" -B "$BUILD" -G "$gen" "${opts[@]}" \
            -DLLVM_ENABLE_PROJECTS="clang;lld" \
            -DLLVM_TARGETS_TO_BUILD=X86 \
            -DLLVM_DEFAULT_TARGET_TRIPLE=x86_64-unknown-linux-musl \
            -DLLVM_ENABLE_ZLIB=ON \
            -DLLVM_ENABLE_ZSTD=OFF \
            -DLLVM_ENABLE_LIBXML2=OFF \
            -DLLVM_ENABLE_TERMINFO=OFF \
            -DLLVM_PARALLEL_LINK_JOBS=4 \
            -DCLANG_DEFAULT_LINKER=lld \
            -DCLANG_DEFAULT_RTLIB=compiler-rt \
            -DCLANG_DEFAULT_CXX_STDLIB=libc++ \
            -DCLANG_DEFAULT_UNWINDLIB=none
    fi
}

build() {
    echo "== building with $JOBS jobs (variant $VARIANT)"
    if [ "$VARIANT" = card ]; then
        cmake --build "$BUILD" -j "$JOBS" --target clang lld llvm-ar llvm-ranlib llvm-nm llvm-objdump llvm-strip llvm-readelf llvm-objcopy
        return
    fi
    if [ "$VARIANT" = dylib ]; then
        cmake --build "$BUILD" -j "$JOBS" --target LLVM
    else
        cmake --build "$BUILD" -j "$JOBS"
    fi
}

install() {
    echo "== installing to $PREFIX"
    if [ "$VARIANT" = card ]; then
        echo "== card variant: not installed; card/userland/components/clang.sh packages $BUILD/bin"
        ls -l "$BUILD/bin/clang-"* "$BUILD/bin/lld" 2>/dev/null | head -3
        return
    fi
    if [ "$VARIANT" = dylib ]; then
        # Only the shared library is wanted; a full install would build every tool.
        mkdir -p "$PREFIX/lib"
        cp -a "$BUILD"/lib/libLLVM*.so* "$PREFIX/lib/"
        ls -l "$PREFIX/lib/" | grep libLLVM
        return
    fi
    cmake --install "$BUILD"
    "$PREFIX/bin/clang" --version | head -1
}

# check: compile the two probe files from docs/research/abi-and-toolchain.md
# with the wrapper and audit the result. The dylib variant ships no clang;
# its check is that rustc loads it, done by toolchain/rust/build-std.sh.
check() {
    if [ "$VARIANT" = card ]; then
        # The card binaries run on the host (same instruction subset): audit
        # clang itself, then compile a probe with it against the sysroot.
        "$phi_root/host/target/debug/phi-isa-audit" --ignore XSAVE "$BUILD/bin/clang-22" | tail -1
        local tmp; tmp=$(mktemp -d)
        printf '#include <stdio.h>\nint main(void){double d=2.5;printf("%%g %%d\\n",d*d,__LINE__);return 0;}\n' > "$tmp/h.c"
        "$BUILD/bin/clang" --config "$phi_root/card/userland/components/clang.cfg" --sysroot="$PHI_SYSROOT" -resource-dir "$PHI_LLVM/lib/clang/22" -o "$tmp/h" "$tmp/h.c"
        "$phi_root/host/target/debug/phi-isa-audit" "$tmp/h" | tail -1
        "$tmp/h"
        rm -rf "$tmp"
        return
    fi
    if [ "$VARIANT" = dylib ]; then
        echo "== check: dylib variant has no clang; verified by toolchain/rust/build-std.sh"
        return
    fi
    local tmp; tmp=$(mktemp -d)
    printf 'int f(int a,int b,int c){return c?a:b;}\nlong g(long a,long b,int c){return c?a:b;}\n' > "$tmp/t.c"
    printf 'double m(double a,double b){return a*b+1.5;}\nfloat h(float a){return a/3.0f;}\n' > "$tmp/d.c"
    PHI_LLVM="$PREFIX" "$phi_root/toolchain/clang/knc-cc" -O2 -c "$tmp/t.c" -o "$tmp/t.o" -nostdinc -nostdlib
    PHI_LLVM="$PREFIX" "$phi_root/toolchain/clang/knc-cc" -O2 -c "$tmp/d.c" -o "$tmp/d.o" -nostdinc -nostdlib
    "$phi_root/host/target/debug/phi-isa-audit" "$tmp/t.o"
    "$phi_root/host/target/debug/phi-isa-audit" "$tmp/d.o"
    "$PREFIX/bin/llvm-objdump" -d --no-show-raw-insn "$tmp/d.o" | grep -E "fld|fmul|fadd|fstp|ret" | head -8
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
