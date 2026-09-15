/*
 * pi-bbp.c: hexadecimal digits of pi by the Bailey-Borwein-Plouffe digit
 * extraction formula, a CPU benchmark for the card and the host.
 *
 *   pi = sum_{k>=0} 16^-k (4/(8k+1) - 2/(8k+4) - 1/(8k+5) - 1/(8k+6))
 *
 * Multiplying by 16^n and keeping fractional parts gives the hex digits
 * from position n on without the digits before it (Bailey, Borwein,
 * Plouffe, "On the rapid computation of various polylogarithmic
 * constants", Math. Comp. 66, 1997). Every 6-digit block is independent, so
 * the blocks are handed to N threads through one counter, largest position
 * first (the cost of a block grows with its position, so the expensive
 * ones must not land in the tail). The sums run in x87 extended precision
 * (64-bit mantissa) on both machines. The fraction left after a block's
 * digits is compared with the next block's starting fraction, computed
 * independently: their gap measures the rounding error at the scale of
 * the last kept digit, and every block whose remainder lies closer to a
 * digit boundary than that error is reported as uncertain. Compiled on the
 * card by its own clang:
 *   cc -O2 -o pi-bbp pi-bbp.c -lpthread
 *   ./pi-bbp DIGITS THREADS [out.txt]
 * See pi-bbp.md.
 */
#include <math.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define MAX_THREADS 512
#define BLOCK 6 /* digits per work unit: 24 bits, well inside the 64-bit mantissa */

static long NDIGITS, nblocks;
static char *digits;            /* NDIGITS hex digits after the point */
static long double *head, *rem; /* per block: fraction at its start, after its digits */
static volatile long next_block;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static uint64_t terms[MAX_THREADS];

static double now(void)
{
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec + ts.tv_nsec * 1e-9;
}

#ifdef BARRETT
/*
 * Variant selected with -DBARRETT: a*b mod m by Barrett reduction with a
 * 64-bit reciprocal mu = floor((2^64 - 1) / m) computed once per modulus.
 * q = (p * mu) >> 64 underestimates floor(p / m) by at most two for
 * p < 2^64 and m < 2^32, so r = p - q*m needs at most two corrections. This
 * path uses the multiplier only; the default path uses the integer
 * divider about eighteen times per term.
 */
static inline uint64_t mulmod(uint64_t a, uint64_t b, uint64_t m, uint64_t mu)
{
	uint64_t p = a * b;
	uint64_t q = (uint64_t)(((unsigned __int128)p * mu) >> 64);
	uint64_t r = p - q * m;
	while (r >= m)
		r -= m;
	return r;
}

/* 16^e mod m, exact: m < 2^32 keeps every product below 2^64. */
static uint64_t modpow16(uint64_t e, uint64_t m)
{
	uint64_t mu = UINT64_MAX / m;
	uint64_t r = 1, b = 16 % m;
	while (e) {
		if (e & 1)
			r = mulmod(r, b, m, mu);
		b = mulmod(b, b, m, mu);
		e >>= 1;
	}
	return r;
}
#else
/* 16^e mod m, exact: m < 2^32 keeps every product below 2^64. */
static uint64_t modpow16(uint64_t e, uint64_t m)
{
	uint64_t r = 1, b = 16 % m;
	while (e) {
		if (e & 1)
			r = r * b % m;
		b = b * b % m;
		e >>= 1;
	}
	return r;
}
#endif

/* Fractional part of sum_{k>=0} 16^(n-k) / (8k+j), in x87 extended precision. */
static long double series(int j, long n, uint64_t *count)
{
	long double s = 0;
	for (long k = 0; k < n; k++) {
		uint64_t m = 8 * (uint64_t)k + j;
		s += (long double)modpow16(n - k, m) / m;
		s -= (long)s;
	}
	/* Terms with k >= n are plain powers of 1/16; stop below the resolution. */
	long double t = 1;
	for (long k = n;; k++) {
		long double term = t / (8 * (long double)k + j);
		if (term < 1e-20L)
			break;
		s += term;
		t /= 16;
	}
	*count += n;
	return s - (long)s;
}

static void *worker(void *arg)
{
	long id = (long)arg;
	uint64_t count = 0;
	static const char hex[] = "0123456789ABCDEF";

	for (;;) {
		pthread_mutex_lock(&lock);
		long b = next_block++;
		pthread_mutex_unlock(&lock);
		if (b >= nblocks)
			break;
		b = nblocks - 1 - b; /* largest position first */
		long n = b * BLOCK;
		long double x = 4 * series(1, n, &count) - 2 * series(4, n, &count) -
				series(5, n, &count) - series(6, n, &count);
		x -= floorl(x);
		head[b] = x;
		for (int i = 0; i < BLOCK; i++) {
			x *= 16;
			int d = (int)x;
			x -= d;
			if (n + i < NDIGITS)
				digits[n + i] = hex[d];
		}
		rem[b] = x;
	}
	terms[id] = count;
	return NULL;
}

int main(int argc, char **argv)
{
	if (argc < 3) {
		fprintf(stderr, "usage: %s DIGITS THREADS [out.txt]\n", argv[0]);
		return 2;
	}
	NDIGITS = atol(argv[1]);
	int threads = atoi(argv[2]);
	if (NDIGITS < 1 || threads < 1 || threads > MAX_THREADS)
		return 2;
	nblocks = (NDIGITS + BLOCK - 1) / BLOCK;
	digits = malloc(NDIGITS + 1);
	head = malloc(nblocks * sizeof *head);
	rem = malloc(nblocks * sizeof *rem);
	if (!digits || !head || !rem)
		return 1;

	static pthread_t th[MAX_THREADS];
	double t0 = now();
	for (long i = 0; i < threads; i++)
		if (pthread_create(&th[i], NULL, worker, (void *)i))
			return 1;
	uint64_t total = 0;
	for (int i = 0; i < threads; i++) {
		pthread_join(th[i], NULL);
		total += terms[i];
	}
	double dt = now() - t0;
	digits[NDIGITS] = 0;

	/*
	 * Certification. rem[b] and head[b+1] are the same fraction computed by
	 * two independent blocks, so their gap measures the error at the scale
	 * of the last kept digit (a gap near 1 is a gap near 0 seen across a
	 * carry). A block whose remainder lies closer to a digit boundary than
	 * sixteen times the worst gap seen could have its last digit wrong; it
	 * is counted as uncertain.
	 */
	long double worst = 0;
	for (long b = 0; b + 1 < nblocks; b++) {
		long double d = fabsl(rem[b] - head[b + 1]);
		if (d > 0.5L)
			d = 1 - d;
		if (d > worst)
			worst = d;
	}
	long bad = 0;
	for (long b = 0; b < nblocks; b++) {
		long double edge = rem[b] < 0.5L ? rem[b] : 1 - rem[b];
		if (edge < 16 * worst)
			bad++;
	}
	/* The first hex digits of pi, 3.243F6A88...: BBP paper, section 4. */
	static const char known[] = "243F6A8885A308D313198A2E03707344A4093822299F31D0082EFA98EC4E6C89";
	int prefix_ok = strncmp(digits, known, NDIGITS < 64 ? NDIGITS : 64) == 0;

	if (argc >= 4) {
		FILE *f = fopen(argv[3], "w");
		if (!f)
			return 1;
		fprintf(f, "3.%s\n", digits);
		fclose(f);
	}
	printf("%ld hex digits, %d threads: %.3f s, %.1f M terms/s, first 32: 3.%.32s, prefix %s, "
	       "uncertain blocks %ld, worst neighbour gap %.1Le\n",
	       NDIGITS, threads, dt, total / dt / 1e6, digits, prefix_ok ? "ok" : "WRONG", bad, worst);
	return bad || !prefix_ok;
}
