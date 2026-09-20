# xz

XZ Utils (`liblzma` plus the `xz` driver) for the card, static, threaded.
`xz.sh` pins the version to whatever the host runs so a card-versus-host
measurement compares the same algorithm rather than two different ones;
5.8.3 at the time of writing.

Measured results are in `docs/results/2026-09-19-xz.md`. Short version: the
card reaches about a quarter of the host's throughput, and the default
settings give a twentieth of that until thread count and block size are
tuned.

## Three things that had to be worked around

**The range decoder ships x86-64 assembly guarded on the wrong thing.**
`src/liblzma/rangecoder/range_decoder.h` sets
`LZMA_RANGE_DECODER_CONFIG` to `0x1F0`, enabling hand-written inline
assembly, whenever `__x86_64__` is defined. That macro is being used as a
proxy for "has CMOV", which is exactly the assumption this card breaks. The
assembly contains `cmov` literally, so `-mno-cmov` cannot help and
`--disable-assembler` does not reach it: `phi-isa-audit` reported 216
illegal instructions, every one of them in `lzma_decoder.o`. The build
passes `-DLZMA_RANGE_DECODER_CONFIG=0`, selecting the portable C path that
every non-x86 target already compiles.

**`--disable-shared` does not make the program static.** It governs
`liblzma` only; the `xz` driver still links against the dynamic loader. The
card has no loader, so the result fails at exec with a bare
`sh: /opt/phi/bin/xz: not found`, which reads like a missing file rather
than a link problem. Worth remembering for any future autotools component.

**libtool filters `-static` out of the program link.** It reads `-static`
as "prefer static libtool libraries", not "produce a static binary", and
its own `-all-static` is rejected by configure's plain compiler test, which
invokes clang directly. Passing it through `CC` does not survive either,
because libtool reconstructs the link line. `xz.sh` therefore passes
`LDFLAGS=-all-static` at make time and then *verifies* the result with
`llvm-readelf -l | grep INTERP`, relinking by hand if a dynamic binary came
out anyway. The check matters more than the flag: three of the four
approaches silently produced a dynamic binary that built cleanly and
audited cleanly.

## Using it

```sh
card/userland/components/xz.sh
phi put ~/.cache/intel-phi-3120a-build/userland/xz/xz /opt/phi/bin/xz
phi run sh -c 'chmod 755 /opt/phi/bin/xz; ln -sf /opt/phi/bin/xz /usr/bin/xz'
```

`init` links `/opt/phi/bin` into `/usr/bin` at every boot, so after the
next card restart the symlink step is unnecessary. Note that busybox also
provides an `xz` applet, decompression only; `/opt/phi/bin` comes first on
the login PATH and `/usr/bin` before `/bin` on the default one, so the real
xz wins in both.

`liblzma.a` is installed into the card sysroot as well, for anything else
built for the card that wants compression.

## Tuning, if you do run it here

Block count, not thread count, is the parallelism limit: xz gives each
thread a whole block, and the default block size is three times the
dictionary. At `-6` that is 24 MiB, so a 473 MiB input makes 19 blocks and
at most 19 of 228 threads can run. `--block-size` buys threads at a small
cost in ratio. Two threads per core is the peak, matching the independent
bandwidth measurement in `card/examples/membw.md`; `-T228` measured worse
than `-T114`. High presets are capped by memory instead: `-9` wants roughly
700 MiB per thread, so about seven threads fit in the card's 6 GB.
