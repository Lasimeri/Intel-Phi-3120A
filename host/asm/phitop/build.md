# host/asm/phitop/build.sh

Assembles phitop's sources and the three it shares with phictl
(`../phictl/text.S`, `sysfs.S`, `wire.S`, each including this
directory's `defs.inc`, which includes phictl's) into
`host/asm/out/obj-phitop/`, and links them static into
`host/asm/out/phitop` (or `DIR/phitop` with `--out DIR`). `make build`
runs it and installs the binary at `host/target/debug/phitop` by copy
and rename, the path `phi top` and the family use.
