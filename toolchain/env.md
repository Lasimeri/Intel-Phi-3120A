# toolchain/env.sh

Sourced by the toolchain build scripts. It solves one problem: the
repository lives in a directory whose name contains spaces (`Intel Phi
3120A`), and third-party build systems expand `$CC`, `$DESTDIR`, and
similar variables unquoted (musl's `configure` line 249 is the first one
that broke). Rather than patching every build system, the scripts operate
through a space-free symlink, `~/.cache/intel-phi-3120a`, that points at
the repository root (`$XDG_CACHE_HOME` is honored).

It also puts `toolchain/clang` (the `knc-cc` wrapper) and the patched
LLVM's `bin` on `PATH`, and exports `CC=knc-cc`, so build systems see a
bare command name rather than a path.

Everything the scripts build ends up under the real repository directory,
because the symlink resolves there; only the paths the scripts hand to
third-party tools differ.
