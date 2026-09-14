# toolchain/env.sh

Sourced by the toolchain and card build scripts. It exists because the
repository directory name contains spaces (`Intel Phi 3120A`) and
third-party build systems do not cope:

| Problem | Symptom | Solution |
| --- | --- | --- |
| `$CC`, `$DESTDIR` expanded unquoted | musl `configure` line 249: `/home/lasimeri/Intel: No such file` | Scripts use `phi_root`, a space-free symlink to the repository (`~/.cache/intel-phi-3120a`), and a bare `CC=knc-cc` found on `PATH` |
| `make` computes `CURDIR` with `getcwd()`, which returns the real path even inside a symlinked tree | busybox `Makefile:279: /home/lasimeri/Intel: No such file` | The build trees are real directories under `~/.cache/intel-phi-3120a-build/` and `toolchain/build`, `card/kernel/build`, `card/userland/build`, `card/initramfs/build` are symlinks to them |

`env.sh` creates both on first use, migrates an older real `build/`
directory into the cache location, and exports `phi_root`, `phi_build`,
`PHI_LLVM`, `PHI_SYSROOT`, `PATH` (wrapper and patched LLVM first), `CC`,
`CXX`. `XDG_CACHE_HOME` and `PHI_BUILD_ROOT` override the locations.

On a host whose checkout path has no spaces none of this is needed, and
the script degrades to plain exports plus the symlinks (which are harmless).
