# clang.cfg

Default options for the card's own clang, installed next to the binary as
`clang.cfg`, `clang++.cfg`, `cc.cfg` and `c++.cfg` so that every driver name
picks them up: the same instruction-set restrictions as the host wrapper
`toolchain/clang/knc-cc`, plus `--sysroot=/opt/phi`, static linking,
compiler-rt and libc++. See `clang.md`.
