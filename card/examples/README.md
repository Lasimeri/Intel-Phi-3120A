# card/examples

Programs compiled for the card and run on it. None of them is built by
`make`: each is compiled either on the host with `knc-cc` and pushed, or on
the card itself with the native clang, and each doc says which. They exist
to produce the numbers in `docs/results/`, not as a library.

| Program | What it measures or proves | Record |
| --- | --- | --- |
| `mandel.c` | Double-precision throughput on x87, threaded, with a parallel deflate stage so the output is a real PNG. The whole card is 0.25 of this host's 16 threads | `docs/results/2026-09-15-mandelbrot.md` |
| `pi-bbp.c` | 64-bit integer and modular-exponentiation throughput, no floating point. 0.14 of the host | `docs/results/2026-09-15-pi-bbp.md` |
| `mandel-vpu.c` with `mandel_vpu.S` | The same image on the 512-bit vector unit, 8 doubles per instruction, against AVX2 on the host. 1.47x on the iteration pass | `docs/results/2026-09-15-vpu.md` |
| `vpu_probe.c` with `vpu_probe.S` | That the MVEX encoder produces instructions the card executes correctly: every lane of every result checked against scalar arithmetic, including masking and the `zmm31` register extension bits | `docs/results/2026-09-15-vpu.md` |
| `vpu_state.c` | That kernel patch 0024 saves and restores vector state across context switches: 32 zmm plus 8 mask registers per thread, compared after every sleep | `docs/results/2026-09-15-vpu.md` (0 mismatches in 3.7 M checks) |
| `phiperf.c` | Hardware performance counters through `perf_event_open` against the mainline KNC PMU driver, since the `perf` tool is not built for the card | `docs/results/2026-09-16-sensors.md` |
| `memhog.c` | That host RAM as swap works under real pressure: 8 GiB allocated, touched and read back on a card with 5669 MiB of GDDR5, 0 mismatches | `docs/results/2026-09-19-card-os.md` |
| `membw.c` | Reachable GDDR5 read bandwidth: 80.5 GB/s at 2 threads per core, against 35.7 GB/s on the host. The one axis where the card wins without hand-written MVEX | `card/examples/membw.md` |

The two `.S` files are generated, not written: `knc-mvex-gen mandel` and
`knc-mvex-gen probe` emit them from the encoder in
`host/crates/knc-mvex/`. Regenerate rather than edit.

## Getting one onto the card

Either route works; the second needs no rebuild of the boot image.

```sh
# compiled on the host, carried in the initramfs
knc-cc -O2 -o card/initramfs/extra/opt/mandel card/examples/mandel.c -lpthread -lz
card/initramfs/build.sh          # audits it on the way in

# compiled on the host, pushed to a running card
knc-cc -O2 -o /tmp/pi-bbp card/examples/pi-bbp.c -lpthread
phictl put /tmp/pi-bbp /opt/phi/bin/pi-bbp && phictl exec /opt/phi/bin/pi-bbp 1000

# compiled on the card itself, with the native clang
phictl put card/examples/pi-bbp.c /tmp/pi-bbp.c
phictl exec sh -c 'cd /tmp && cc -O2 -o pi-bbp pi-bbp.c -lpthread && ./pi-bbp 1000'
```

`mandel.c` links zlib (`-lz -lpthread`), so `card/userland/components/zlib.sh`
has to have run before it is compiled. The clang package copies whatever
the sysroot holds at packaging time, so `/opt/phi` carries `libz.a` only if
zlib was built before `clang.sh`. `docs/howto/build-and-run.md` covers the
three routes in full.
