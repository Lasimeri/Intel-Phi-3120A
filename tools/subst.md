# tools/subst.c

`subst FILE OLD NEW`: every occurrence of the literal text `OLD` in `FILE`
becomes `NEW`, in place. The card component scripts call it through
`phi_subst` ([`../toolchain/env.md`](../toolchain/env.md)) to edit the
upstream sources they fetch (FastLanes, QuickJS), where they used `perl -pi`
until 2026-10-08.

- **Literal, not a pattern.** `\n`, `\t` and `\\` in `OLD` and `NEW` stand
  for a newline, a tab and a backslash; every other character is itself.
  A whole-line edit writes the newlines it means: `OLD` `\nfoo\n` is the
  line `foo`, nothing longer or shorter.
- **Missing text is an error.** Exit 1 with "the text to replace is not
  there" when `OLD` does not occur: an upstream version that moved or
  changed the line stops the build instead of building without the edit.
  The scripts guard each call with a `grep` for the edit's own result, so
  a second run over an edited tree does not call it.
- **Safe on failure.** The result is written to `FILE.subst` beside the
  file, given the file's mode, and renamed over it; a failed run leaves
  the file as it was.
- **Output.** `subst: FILE: N replaced` on standard error; exit 2 on a
  usage or system error.

The switch from perl was checked by running each script's fetch and patch
steps twice from the pristine tarballs, once with the perl edits and once
with `subst`: the FastLanes and QuickJS trees came out byte-identical
(`diff -r`), every edit applied exactly once.
