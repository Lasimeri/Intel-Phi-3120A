#!/usr/bin/env bash
# clang.sh: package the card's native clang (PHI_LLVM_VARIANT=card build of
# toolchain/llvm/build.sh) with the sysroot it needs into one tarball,
# /opt/phi on the card. Output: card/userland/build/clang/phi-clang.tar.gz.
# See clang.md; clang-push.sh loads it onto a running card.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
BUILD="${PHI_LLVM_BUILD:-$phi_build/toolchain/llvm-build-card}"
OUT="$root/card/userland/build/clang"
PKG="$OUT/root/opt/phi"
AUDIT="$root/host/target/debug/phi-isa-audit"
HOST_RES="$PHI_LLVM/lib/clang/22"
[ -x "$BUILD/bin/clang-22" ] || { echo "clang.sh: $BUILD/bin/clang-22 missing; run PHI_LLVM_VARIANT=card toolchain/llvm/build.sh configure build" >&2; exit 1; }
rm -rf "$OUT/root"
mkdir -p "$PKG/bin" "$PKG/usr" "$PKG/lib/clang/22/lib/linux"
# Tools: clang (one binary, driver modes by name), lld (one binary), and the
# binutils-style tools. Symlinks keep the tarball small.
cp "$BUILD/bin/clang-22" "$PKG/bin/clang-22"
for n in clang clang++ cc c++; do ln -sfn clang-22 "$PKG/bin/$n"; done
cp "$BUILD/bin/lld" "$PKG/bin/lld"
for n in ld.lld ld; do ln -sfn lld "$PKG/bin/$n"; done
for t in llvm-ar llvm-ranlib llvm-nm llvm-objdump llvm-strip llvm-readelf llvm-objcopy; do
    cp "$BUILD/bin/$t" "$PKG/bin/$t"
    ln -sfn "$t" "$PKG/bin/${t#llvm-}"
done
"$PHI_LLVM/bin/llvm-strip" "$PKG/bin/clang-22" "$PKG/bin/lld" "$PKG"/bin/llvm-*
# The same defaults for every driver name (clang looks for <name>.cfg next
# to the binary).
for n in clang clang++ cc c++; do cp "$here/clang.cfg" "$PKG/bin/$n.cfg"; done
# Sysroot: musl headers and libraries and libc++ under usr/ (clang's driver
# searches <sysroot>/usr/include and <sysroot>/usr/lib), the compiler's own
# headers and compiler-rt in the resource directory it expects
# (../lib/clang/22 relative to the binary).
cp -a "$PHI_SYSROOT/usr/include" "$PKG/usr/include"
cp -a "$PHI_SYSROOT/usr/lib" "$PKG/usr/lib"
cp -a "$HOST_RES/include" "$PKG/lib/clang/22/include"
cp "$HOST_RES"/lib/linux/libclang_rt.builtins-x86_64.a "$HOST_RES"/lib/linux/clang_rt.crt*-x86_64.o "$PKG/lib/clang/22/lib/linux/"
# XSAVE: llvm::sys::getHostCPUName executes xgetbv only after CPUID reports
# OSXSAVE, which the card never does; the instruction is present, not
# reachable. Everything else must be clean (BLAKE3 is built without its
# SSE/AVX assembly, LLVM_DISABLE_ASSEMBLY_FILES).
echo "== audit"
for b in clang-22 lld llvm-ar; do "$AUDIT" --ignore XSAVE "$PKG/bin/$b" | tail -1; done
echo "== probe: the packaged clang runs on the host too"
tmp=$(mktemp -d)
printf '#include <stdio.h>\nint main(void){double d=2.5;printf("%%g\\n",d*d);return 0;}\n' > "$tmp/h.c"
"$PKG/bin/clang" --sysroot="$PKG" -o "$tmp/h" "$tmp/h.c"
"$AUDIT" "$tmp/h" | tail -1
"$tmp/h"
rm -rf "$tmp"
(cd "$OUT/root" && tar -czf "$OUT/phi-clang.tar.gz" opt)
ls -l "$OUT/phi-clang.tar.gz"
du -sh "$PKG" | cut -f1
