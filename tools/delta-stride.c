/* stride.c: what the interleaved layout costs DELTA.
 *
 * Value i lives in lane i % 16, so "the value before it in the same lane"
 * is the value 16 positions earlier in the caller's array, not its
 * neighbour. For a sorted column that is a stride-16 difference where a
 * transposed layout would give a stride-1 one, and the packed width follows
 * the largest difference. This measures how many bits that costs.
 *
 *   tcc -run stride.c
 */
#include <stdio.h>
#include <stdlib.h>

#define BLOCK 1024
#define LANES 16

static unsigned bits_for(unsigned v)
{
	unsigned n = 0;

	while (v) { n++; v >>= 1; }
	return n;
}

/* xorshift, so the shape of the data is reproducible and not libc's. */
static unsigned rnd(unsigned *s)
{
	unsigned x = *s;

	x ^= x << 13; x ^= x >> 17; x ^= x << 5;
	return *s = x;
}

static void report(const char *what, unsigned *v)
{
	unsigned max1 = 0, max16 = 0;
	int i;

	for (i = 1; i < BLOCK; i++)
		if (v[i] - v[i - 1] > max1)
			max1 = v[i] - v[i - 1];
	for (i = LANES; i < BLOCK; i++)
		if (v[i] - v[i - LANES] > max16)
			max16 = v[i] - v[i - LANES];

	printf("%-28s %10u %6u %10u %6u %8u\n", what,
	       max1, bits_for(max1), max16, bits_for(max16),
	       bits_for(max16) - bits_for(max1));
}

int main(void)
{
	static unsigned v[BLOCK];
	unsigned s = 0x9E3779B9, acc;
	int i;

	printf("%-28s %10s %6s %10s %6s %8s\n", "column", "max d1", "bits",
	       "max d16", "bits", "cost");

	/* Dense identifiers: mostly +1, occasional gap. */
	acc = 1000000;
	for (i = 0; i < BLOCK; i++) { v[i] = acc; acc += 1 + (rnd(&s) % 3 == 0); }
	report("dense ids, gaps 1 to 2", v);

	/* Timestamps in milliseconds, arrivals a few ms apart. */
	acc = 1700000000u;
	for (i = 0; i < BLOCK; i++) { v[i] = acc; acc += 1 + rnd(&s) % 20; }
	report("timestamps, 1 to 20 ms", v);

	/* A sorted key column with a wider spread. */
	acc = 0;
	for (i = 0; i < BLOCK; i++) { v[i] = acc; acc += 1 + rnd(&s) % 1000; }
	report("sorted keys, gaps to 1000", v);

	/* Strictly sequential: the best case for delta anywhere. */
	for (i = 0; i < BLOCK; i++)
		v[i] = 5000000 + (unsigned)i;
	report("strictly sequential", v);

	printf("\nBits are what the block's widest difference needs, which is what\n");
	printf("the packed width has to be. 'cost' is the extra bits per value the\n");
	printf("interleaved layout asks for against a transposed one.\n");
	return 0;
}
