# mandel.c

A Mandelbrot renderer used as the card's CPU benchmark. Two stages, both
spread over N threads through one mutex-protected work counter:

1. Render: double precision escape-time iteration (x87 on Knights Corner),
   one row per grab, smooth colouring, written straight into the PNG
   scanline buffer (a filter byte then RGB per row, so no copy later).
2. Deflate: the scanline buffer cut into 128 KiB pieces, one piece per
   grab, compressed the way pigz does it (pigz.c, "Parallel deflate"):
   every piece gets its own raw deflate state (windowBits -15, no zlib
   framing) primed with the 32 KiB that precede it (`deflateSetDictionary`,
   so the decoder's sliding window matches and the ratio stays within a
   percent of a single stream), and ends with `Z_SYNC_FLUSH`, which emits
   an empty stored block and leaves the output byte aligned without a
   final-block bit; the last piece ends with `Z_FINISH`. Concatenated, the
   pieces are one valid deflate stream. The zlib header, the adler32
   trailer (`adler32_combine` over the per piece values) and the IDAT
   chunk CRC (`crc32_combine` likewise) are assembled without rescanning
   the data, so the only serial work left is the `fwrite`.

Compiled on the card by its own clang:

```
cc -O2 -o mandel mandel.c -lz -lpthread
./mandel out.png 1920 1080 2000 228            # full set
./mandel out.png 7680 4320 2000 228            # 8K
./mandel out.png 1920 1080 4000 228 -0.7453 0.1127 0.006   # a detail
```

Prints render time, megapixels and giga-iterations per second, the deflate
time with the piece count and the byte counts, and the write time.

Notes:

- Memory: the scanline buffer is `H * (3 * W + 1)` bytes (99.5 MB at 8K)
  and the compressed pieces are allocated at `deflateBound` each, so the
  peak is about twice the raw image plus 256 KiB of deflate state per
  thread.
- zlib's `crc32()` with a null buffer returns the initial value instead of
  the running CRC, so `chunk()` skips the data call for an empty chunk
  (the IEND CRC was wrong before that guard).
- `MAX_THREADS` is 512; the card runs 228, the host 16.
- The card's images differ from the host's in a few thousand boundary
  pixels (about 1 in 14,000 at 8K): clang keeps x87 intermediates in
  extended precision, SSE2 rounds to double, and pixels whose escape
  iteration lands on that difference get a different colour.

Results: `docs/results/2026-09-15-mandelbrot.md`.
