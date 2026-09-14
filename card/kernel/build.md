# card/kernel/build.sh

Builds the card kernel the same way on every machine:

| Step | What it does |
| --- | --- |
| `fetch` | Shallow clone of the stable tree at the tag on the first line of `patches/SERIES` into `card/kernel/build/linux` (a symlink into `~/.cache/intel-phi-3120a-build/kernel`, see `toolchain/env.md`). |
| `patch` | `git reset --hard <tag>`, `git clean`, then `git am` of every patch in `SERIES` order, so the tree never depends on what was applied before. |
| `configure` | `x86_64_defconfig`, then `scripts/kconfig/merge_config.sh` with `config/knc.config`, then `olddefconfig`; fails if `CONFIG_X86_KNC` is missing (patches not applied) or if any line of the fragment did not survive. |
| `build` | `make LLVM=1 LLVM_IAS=1 CC=knc-cc ... bzImage` with `O=card/kernel/build/out`. |
| `audit` | `phi-isa-audit vmlinux` with a documented exception per reason (the `--ignore` list in the script, one comment each): every one is an instruction that sits behind a runtime CPUID or erratum check the static audit cannot follow. `.altinstr_replacement` is skipped by the tool itself. Anything not on that list fails the build. |

## Why `CC=knc-cc`

`LLVM=1` selects clang, lld and the llvm binutils from `PATH`, where
`toolchain/env.sh` has put the patched install first. The compiler must
still be told to avoid CMOV and the rest of the deletion list, and
`KCFLAGS` does not reach the decompressor: `arch/x86/boot/compressed/Makefile`
replaces `KBUILD_CFLAGS` wholesale. The wrapper carries the flags in the
compiler itself, so every object, the decompressor included, is compiled
alike. Host programs (kconfig, objtool, scripts) are compiled with
`/usr/bin/clang`; the patched clang defaults to the musl target and would
not link them.

## Output

`card/kernel/build/out/arch/x86/boot/bzImage` for `phictl boot`, and
`card/kernel/build/out/vmlinux` for the audit and for symbolizing
addresses in the console output.

## Environment

`PHI_KERNEL_TAG`, `PHI_KERNEL_SRC`, `PHI_KERNEL_OUT`, `PHI_JOBS` override
the defaults; `env.sh` provides the paths. `git am` uses a fixed local
identity so the tree does not depend on the user's git configuration.
