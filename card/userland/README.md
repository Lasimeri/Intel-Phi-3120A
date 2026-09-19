# card/userland

Per-component notes for what runs on the card. Each file records: the
upstream version pinned, the configure flags, the ABI-sensitive spots
(anything that touches float passing or contains assembly), the audit
result, and the smoke test.

Seven components have a build script here. The rest are notes: either the
work happens elsewhere (musl is built by `toolchain/musl/build.sh` as part
of the cross toolchain) or the component has been reasoned about but not
built.

| Component | Script | Language | What it is |
| --- | --- | --- | --- |
| [busybox](components/busybox.md) | `busybox.sh` | C | 1.37.0, static against musl; the shell, the applets, and `init`'s entire vocabulary |
| [dropbear](components/dropbear.md) | `dropbear.sh` | C | SSH server; host keys generated on the host so the fingerprint is stable across card boots |
| [zlib](components/zlib.md) | `zlib.sh` | C | 1.3.1 into the card sysroot; first consumer is CPython |
| [ncurses](components/ncurses.md) | `ncurses.sh` | C | 6.5 wide-character, static, terminfo compiled in |
| [CPython](components/cpython.md) | `cpython.sh` | C | 3.14 static; what the ported htop/glances/fastfetch clones run on |
| [clang](components/clang.md) | `clang.sh` | C++ | the card's own compiler, a Canadian cross of the same patched LLVM |
| [clang-push](components/clang-push.md) | `clang-push.sh` | shell | pushes that tarball to `/opt/phi` over `phictl put`/`exec`, no SSH needed |
| [clang.cfg](components/clang.cfg.md) | (file) | config | the driver defaults installed beside the card's clang so `cc` on the card does the right thing unprompted |
| [musl](components/musl.md) | (toolchain) | C | libc; built by `toolchain/musl/build.sh`, one patch (`a_spin` without `pause`) |
| [init](components/init.md) | (initramfs) | shell | PID 1; the script is `card/initramfs/init`, not a component build |
| [gcc](components/gcc.md) | (notes) | C | the two `i386.cc` amendments a knc64-x87 gcc would need; not built |
| [tcc](components/tcc.md) | (notes) | C | what an x87 float backend would take; not built |
| [QuickJS](components/quickjs.md) | (notes) | C | pure C99, expected to build unmodified; not built |

## Where the output goes

The scripts install into the card sysroot under
`~/.cache/intel-phi-3120a-build/` (reached as `card/userland/build`, see
`toolchain/env.md`). Only busybox, dropbear and `phi-agent` go into the
initramfs. clang and CPython are pushed to `/opt/phi` on the card's
persistent disk instead, which is why the boot image stays near 1.4 MB.
