# mandel.c

A Mandelbrot renderer used as the card's first CPU benchmark: double
precision escape-time iteration (x87 on Knights Corner), rows handed out
dynamically to N threads through a mutex-protected counter, smooth
colouring, PNG output through zlib (deflate level 6, a minimal PNG writer
with its own CRC). Compiled on the card by its own clang:

```
cc -O2 -o mandel mandel.c -lz -lpthread
./mandel out.png 1920 1080 2000 228            # full set
./mandel out.png 1920 1080 4000 228 -0.7453 0.1127 0.006   # a detail
```

Prints render time, megapixels per second and giga-iterations per
second. Results: `docs/results/2026-09-15-mandelbrot.md`.
