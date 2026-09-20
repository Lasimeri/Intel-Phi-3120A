#!/usr/bin/env bash
# quickjs.sh: build QuickJS for the card (ADR 0004), static against musl,
# with the knc module linked in so JavaScript reaches the vector unit.
# Installs qjs and qjsc into a package tree for /opt/phi. See quickjs.md.
#   PHI_QUICKJS_VERSION   default 2026-06-04
# Output: card/userland/build/quickjs/phi-quickjs.tar.gz
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../../toolchain/env.sh"
root="$phi_root"
VER="${PHI_QUICKJS_VERSION:-2026-06-04}"
URL="https://bellard.org/quickjs/quickjs-$VER.tar.xz"
SRC="$root/card/userland/build/quickjs-$VER"
OUT="$root/card/userland/build/quickjs"
AUDIT="$root/host/target/debug/phi-isa-audit"
[ -x "$AUDIT" ] || { echo "quickjs.sh: $AUDIT missing; run 'make build' first" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libc.a" ] || { echo "quickjs.sh: no musl sysroot at $PHI_SYSROOT; run toolchain/musl/build.sh first" >&2; exit 1; }
[ -f "$PHI_SYSROOT/usr/lib/libknc.a" ] || { echo "quickjs.sh: libknc.a not in the sysroot; run card/lib/knc/build.sh first" >&2; exit 1; }
mkdir -p "$OUT"
# Pinned download (toolchain/fetch.sh, toolchain/SHA256SUMS).
phi_fetch "quickjs-$VER.tar.xz" "$URL"
tarball="$phi_fetched"
rm -rf "$SRC"; mkdir -p "$SRC"
tar -xJf "$tarball" -C "$SRC" --strip-components=1
cd "$SRC"

# The knc module, as a QuickJS native module compiled into qjs.
cp "$root/card/lib/knc-js/qjs_knc.c" "$root/card/lib/knc-js/qjs_knc.h" .

# quickjs.c defines js_std_add_helpers and friends; qjs.c is the shell. The
# shell has no hook for "add a module", so the call is inserted where it
# already builds the module list, next to the std and os modules.
if ! grep -q js_init_module_knc qjs.c; then
    perl -0pi -e 's/(\s*js_init_module_os\(ctx, "os"\);)/$1\n        js_init_module_knc(ctx, "knc");/' qjs.c
    perl -0pi -e 's/(#include "quickjs-libc\.h")/$1\n#include "qjs_knc.h"/' qjs.c
fi
grep -n 'js_init_module_knc' qjs.c | sed 's/^/   /'

# Atomics.pause() reaches for the PAUSE instruction through inline assembly
# guarded on __x86_64__, which KNC deletes (ISA reference 327364-001, App.
# B.2). Requiring __SSE2__ as well sends this card to the empty no-op the
# file already has for other architectures, and changes nothing anywhere
# else: PAUSE and SSE2 arrived together and no other x86-64 lacks either.
#
# Third dialect of the same bug: xz guarded CMOV assembly on __x86_64__
# (xz.md), zstd used function target attributes with CPUID dispatch
# (zstd.md), and this is inline assembly again. All three assume x86-64
# implies a feature set.
perl -0pi -e 's/#elif defined\(__x86_64\) \|\| defined\(__i386__\)\n(static inline void cpu_pause\(void\)\n\{\n    asm volatile\("pause")/#elif (defined(__x86_64) || defined(__i386__)) \&\& defined(__SSE2__)\n$1/' quickjs.c
grep -n 'defined(__SSE2__)' quickjs.c | sed 's/^/   /'

# The module object is built by hand and handed to the link through
# EXTRA_LIBS, which the Makefile appends to the link line. QuickJS has no
# hook for adding an object to QJS_OBJS from the command line, and an
# object on the library line links the same way.
knc-cc -O2 -c -o qjs_knc.o qjs_knc.c

# ADR 0004 already says LTO is off. The -mno-* set and -fno-jump-tables
# come from knc-cc itself, so nothing here has to repeat them.
#
# LDFLAGS must carry -static explicitly: knc-cc does not add it (the kernel
# build needs it not to), and without it this links a dynamic PIE, which
# fails on libknc's function tables (absolute 64-bit relocations) long
# before it fails on the card having no dynamic loader.
make -j"$(nproc)" \
    CONFIG_LTO= \
    CC=knc-cc AR=llvm-ar STRIP=llvm-strip \
    LDFLAGS="-static" \
    EXTRA_LIBS="$SRC/qjs_knc.o -lknc" \
    qjs qjsc libquickjs.a > "$OUT/make.log" 2>&1 || { tail -40 "$OUT/make.log"; exit 1; }

echo "== audit (must be clean)"
"$AUDIT" qjs
"$AUDIT" qjsc

echo "== the card binary runs on the host"
./qjs -e 'console.log("qjs", typeof globalThis)'

echo "== installing into the package tree"
rm -rf "$OUT/root"
install -Dm755 qjs "$OUT/root/opt/phi/bin/qjs"
install -Dm755 qjsc "$OUT/root/opt/phi/bin/qjsc"
install -Dm644 libquickjs.a "$OUT/root/opt/phi/usr/lib/libquickjs.a"
install -Dm644 quickjs.h "$OUT/root/opt/phi/usr/include/quickjs.h"
install -Dm644 quickjs-libc.h "$OUT/root/opt/phi/usr/include/quickjs-libc.h"
(cd "$OUT/root" && tar -czf "$OUT/phi-quickjs.tar.gz" opt)
ls -l "$OUT/phi-quickjs.tar.gz"
