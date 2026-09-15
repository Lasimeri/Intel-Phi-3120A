# zlib for the card

`zlib.sh` builds zlib 1.3.1 statically with `knc-cc` and installs
`libz.a` and `zlib.h` into the card sysroot (`toolchain/build/sysroot/usr`).
Pure C, audited clean. First consumers: CPython's `zlib` module (glances,
pip wheels are zip files) and ncurses is not one (it does not need it).
