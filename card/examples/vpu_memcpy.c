/* vpucpy.c: correctness and speed of the VPU block copy against musl's
 * memcpy. Build on the card:  cc -O2 -o vpucpy vpucpy.c vpu_memcpy.S  */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

void *knc_memcpy64(void *dst, const void *src, size_t blocks);

static double now_s(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

int main(void)
{
	size_t arena = 8u << 20;
	unsigned char *a, *b, *c;
	size_t i, k, bad = 0;

	if (posix_memalign((void **)&a, 64, arena) || posix_memalign((void **)&b, 64, arena)
	    || posix_memalign((void **)&c, 64, arena)) { perror("memalign"); return 1; }
	for (i = 0; i < arena; i++) a[i] = (unsigned char)(i * 31u + 7u);

	/* Correctness: every block count from 0 to 64, against memcpy. */
	for (k = 0; k <= 64; k++) {
		memset(b, 0xAA, arena); memset(c, 0xAA, arena);
		knc_memcpy64(b, a, k);
		memcpy(c, a, k * 64);
		if (memcmp(b, c, arena) != 0) { bad++; if (bad < 4) printf("MISMATCH at %zu blocks\n", k); }
	}
	printf("correctness: %s (%zu/65 block counts wrong)\n", bad ? "FAILED" : "OK", bad);
	if (bad) return 1;

	printf("\n%10s %14s %14s %8s\n", "bytes", "musl MB/s", "VPU MB/s", "speedup");
	size_t sizes[] = { 64, 128, 256, 512, 4096, 65536, 1u << 20 };
	for (k = 0; k < sizeof sizes / sizeof sizes[0]; k++) {
		size_t n = sizes[k], blocks = n / 64;
		size_t iters = (arena / n) & ~(size_t)3, rep, reps = (n < 4096) ? 8 : 2;
		double t0, t1, ms, vs;

		t0 = now_s();
		for (rep = 0; rep < reps; rep++)
			for (i = 0; i < iters; i++) memcpy(b + i * n, a + i * n, n);
		t1 = now_s();
		ms = (double)(iters * reps * n) / 1e6 / (t1 - t0);

		t0 = now_s();
		for (rep = 0; rep < reps; rep++)
			for (i = 0; i < iters; i++) knc_memcpy64(b + i * n, a + i * n, blocks);
		t1 = now_s();
		vs = (double)(iters * reps * n) / 1e6 / (t1 - t0);

		printf("%10zu %14.1f %14.1f %7.2fx\n", n, ms, vs, vs / ms);
	}
	return 0;
}
