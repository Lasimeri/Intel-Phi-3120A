# host/asm/phictl/build.sh

Assembles every `.S` in this directory with `as --64` (with `-I` here
for `defs.inc`) into `host/asm/out/obj/` and links them static with
`_start` as the entry into `host/asm/out/phictl` (or `DIR/phictl` with
`--out DIR`). GNU binutils is the whole requirement: no cargo, no
libc. The objects stay beside the binary (nothing is written to `/tmp`).
`make build` runs it after the Cargo build and installs the binary at
`host/target/debug/phictl` by copy and rename, so a daemon running from
that path (a card up under its unit) keeps its old inode and the next
`phi up` gets the new one; `scripts/phi-env.sh` and the family resolve
`phictl` at that path.
