# toolchain/musl/build.sh

Builds the card libc into `toolchain/build/sysroot` with `knc-cc`.
`docs/decisions/0006-musl-libc.md` is the decision; this is the recipe.

## Steps

1. Fetch `musl-<version>.tar.gz` from musl.libc.org (default 1.2.5), record
   or verify its SHA-256 in `toolchain/build/downloads/SHA256SUMS`.
2. Extract fresh (the tree is never patched in place).
3. Delete every `src/math/x86_64/` file that mentions an XMM constraint or
   register, or any deleted mnemonic (`fcomi`, `fucomi`, `fcmov`, `cmov`,
   `pause`, `prefetch`, fences, `clflush`): musl uses an arch file when
   present and the portable C file otherwise, so deleting is the whole
   port. On 1.2.5 this drops `fabs`, `fabsf`, `fma`, `fmaf`, `sqrt`,
   `sqrtf`, `lrint`, `lrintf`, `llrint`, `llrintf` (XMM) and `exp2l.s`
   (`fucomip`); the script prints the list. The remaining arch files use x87 (`fabs`, `sqrtl`,
   `rintl`, the `*l` transcendental functions), `rep movs/stos`
   (`memcpy`, `memset`), or integer code (`setjmp`, `clone`, `syscall`),
   all supported on Knights Corner (ISA reference App. B). `fenv.s` uses
   `ldmxcsr`/`stmxcsr`, which the card supports (App. B.3).
4. `configure --disable-shared --enable-static` with `CC=knc-cc`, then
   `make install` with `DESTDIR` set to the sysroot. Static only until the
   dynamic loader is validated on the card.
5. Copy Arch's `kernel-headers-musl` UAPI headers into the sysroot.
6. Run `phi-isa-audit` on `libc.a`; a single illegal instruction fails the
   build, which is the point.

## Why the audit matters here

Any SSE instruction that survives in `libc.a` ends up in every program.
The audit on the archive checks every member.

## Testing the result on the host

A knc64-x87 binary is an ordinary x86-64 ELF that avoids instructions the
host also has, so `toolchain/check/run.sh` executes it on the host. The
only difference the host would hide is a float ABI mismatch between two
objects, and the check covers that by mixing C and Rust in one binary.

4a. Apply the project patches in `patches/SERIES` (`patches/README.md`).
