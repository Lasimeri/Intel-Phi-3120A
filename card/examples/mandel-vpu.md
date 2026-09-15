# mandel-vpu.c

The Mandelbrot benchmark on the vector units, the same image and palette
as `mandel.c` computed 8 pixels at a time on the card (`mandel_vpu.S`,
hand-encoded Knights Corner vector code) and 4 at a time on the host
(AVX2 intrinsics, `-mavx2 -mfma`). The two inner loops do the same
operations in the same order with round to nearest, one fused
multiply-add each, so the escape counts and exit magnitudes are identical
on both machines.

Three timed passes over the image, all on N threads:

1. iterate: rows handed out dynamically, the kernel called per group of
   lanes; escape count (u16) and |z|^2 at exit (float) stored per pixel.
   The report gives useful iterations per second (sum of escape counts),
   lane iterations per second (trips times lanes) and the lane utilisation
   (their ratio: lanes idle once they have escaped while their group
   continues).
2. colour: the palette of mandel.c through libm, scalar.
3. deflate in 128 KiB pieces (as mandel.c), then write.

```
cc -O2 -o mandel-vpu mandel-vpu.c mandel_vpu.S -lz -lpthread      # card
cc -O2 -mavx2 -mfma -o mandel-vpu mandel-vpu.c -lz -lpthread -lm   # host
./mandel-vpu out.png 7680 4320 2000 228
```

Results: `docs/results/2026-09-15-vpu.md`.
