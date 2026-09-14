# card/kernel/patches

The card kernel's patch series against the mainline tag named on the
first line of `SERIES`. Generated with `git format-patch` from a working
tree; applied by `../build.sh patch` with `git am` in `SERIES` order onto a
hard-reset checkout, so the series is the only source of truth.

Each patch is a self-contained commit with the reasoning in its message,
citing the SSDG section, the ISA reference appendix, or the measurement
that motivated it. `../README.md` has the one-line index and the sites
that were inspected before the series was written.

To change the series: apply it (`../build.sh patch`), commit changes in
`card/kernel/build/linux` (fixups can be squashed with
`GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash <tag>`), then
regenerate:

```sh
git -C card/kernel/build/linux format-patch --no-signature --zero-commit \
    -o card/kernel/patches <tag>..HEAD
```

and rewrite `SERIES`. `--zero-commit` keeps the files free of hashes that
change on every regeneration.
