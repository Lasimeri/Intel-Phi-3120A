# toolchain/musl/patches

One patch against musl 1.2.5 (`SERIES`), applied by
`toolchain/musl/build.sh` before it configures.

| Patch | What and why |
| --- | --- |
| `0001-x86_64-a_spin-without-pause.patch` | musl's `a_spin()` is a bare `pause`. Knights Corner deletes it (ISA reference 327364-001, appendix B.2), so the instruction faults. The patch makes it a compiler barrier, matching what kernel patch 0004 does for `cpu_relax`. |

That is the whole source delta. Everything else musl needs on this target
comes from the build script's drop rule for the SSE assembly under
`src/*/x86_64/`, which is a build-time selection, not a patch:
`toolchain/musl/build.md` lists which files are dropped and what C
fallback replaces each.
