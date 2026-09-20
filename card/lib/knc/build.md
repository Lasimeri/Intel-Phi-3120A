# build.sh (libknc)

Generates, assembles, audits and installs `libknc`. See `knc.md` for what
the library is and what it measures.

Four steps, in order, each of which can fail loudly:

1. **Rebuild the generator.** Always, unless `PHI_KNC_GEN` points at one. A
   stale `knc-mvex-gen` would quietly emit the previous encoder's bytes and
   nothing downstream would notice, because the output is `.byte` lines that
   assemble either way.
2. **Generate.** `knc-mvex-gen codec` writes about 20500 lines: 32 unpack
   kernels, 32 pack kernels, the two dispatch thunks, the two function
   tables and the lane masks. `knc-mvex-gen memcpy` adds the block copy.
   Neither is committed. They are build products the size of compiler
   output, and `card/examples/bitunpack.S` stays committed instead because
   it is the artifact a published measurement refers to.
3. **Assemble and archive** with `knc-cc -c` and `llvm-ar`, then audit.
   The audit must report 0 illegal, and it counts the MVEX instructions it
   recognises (19848 of them) so a silent decode failure shows up as a
   suspiciously low count rather than as nothing.
4. **Install** into `$PHI_SYSROOT/usr/{lib,include}` for `knc-cc`, and into
   a package tree rooted at `/opt/phi/usr` for the card's own clang, which
   is where its `--sysroot=/opt/phi` makes it look.

## Environment

| Variable | Effect |
| --- | --- |
| `PHI_KNC_GEN` | use this generator binary and skip the rebuild |

Needs `toolchain/build/sysroot` populated (`toolchain/musl/build.sh`) and
`make build` for `phi-isa-audit`.
