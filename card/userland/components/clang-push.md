# clang-push.sh

Loads `phi-clang.tar.gz` (built by `clang.sh`) onto a running card through
`phictl put` and `phictl exec` (no SSH): the archive lands in `/tmp`,
unpacks to `/opt/phi`, and a probe is compiled and run on the card. See
`clang.md`.
