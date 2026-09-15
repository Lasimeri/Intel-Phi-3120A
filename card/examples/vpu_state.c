/*
 * vpu_state.c: does the kernel preserve the vector unit's registers across
 * context switches? Every thread loads a pattern unique to it into all 32
 * zmm registers and the 8 mask registers, then repeatedly sleeps, reads
 * the registers back and compares. With two threads per CPU the scheduler
 * switches between them constantly, so any register the kernel fails to
 * save and restore shows up as a mismatch. Compiled on the card:
 *   cc -O2 -o vpu_state vpu_state.c vpu_probe.S -lpthread
 *   ./vpu_state THREADS SECONDS
 * See vpu_state.md.
 */
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define AREA (32 * 64 + 16) /* 32 zmm then 8 x u16 masks; vpu_probe.S layout */

void vpu_set(const void *area); /* load zmm0..31 and k0..k7 from area */
void vpu_get(void *area);       /* store them to area */

static int seconds;
static long mismatches[512], low128[512], masks[512], checks[512];

static void fill(uint8_t *area, long id)
{
	for (int r = 0; r < 32; r++)
		for (int i = 0; i < 8; i++) {
			uint64_t v = 0x1000000000000000ull * (uint64_t)(r + 1) + (uint64_t)id * 0x10000 + (uint64_t)i;
			memcpy(area + r * 64 + i * 8, &v, 8);
		}
	for (int k = 0; k < 8; k++) {
		uint16_t m = (uint16_t)(0x1111 * (k + 1) + id);
		memcpy(area + 2048 + 2 * k, &m, 2);
	}
}

static void *worker(void *arg)
{
	long id = (long)arg;
	uint8_t *want = aligned_alloc(64, AREA + 48);
	uint8_t *got = aligned_alloc(64, AREA + 48);
	struct timespec t0, t;

	fill(want, id);
	clock_gettime(CLOCK_MONOTONIC, &t0);
	for (;;) {
		vpu_set(want);
		usleep(500 + (id % 7) * 100);
		memset(got, 0, AREA);
		vpu_get(got);
		checks[id]++;
		if (memcmp(got, want, AREA)) {
			mismatches[id]++;
			int only_low = 1;
			for (int r = 0; r < 32 && only_low; r++)
				if (memcmp(got + r * 64 + 16, want + r * 64 + 16, 48))
					only_low = 0;
			if (memcmp(got + 2048, want + 2048, 16))
				masks[id]++;
			if (only_low)
				low128[id]++;
		}
		clock_gettime(CLOCK_MONOTONIC, &t);
		if (t.tv_sec - t0.tv_sec >= seconds)
			break;
	}
	return NULL;
}

int main(int argc, char **argv)
{
	if (argc < 3)
		return 2;
	int threads = atoi(argv[1]);
	seconds = atoi(argv[2]);
	if (threads < 1 || threads > 512)
		return 2;
	static pthread_t th[512];
	for (long i = 0; i < threads; i++)
		pthread_create(&th[i], NULL, worker, (void *)i);
	long c = 0, m = 0, l = 0, k = 0;
	for (int i = 0; i < threads; i++) {
		pthread_join(th[i], NULL);
		c += checks[i];
		m += mismatches[i];
		l += low128[i];
		k += masks[i];
	}
	printf("%d threads, %d s: %ld checks, %ld mismatches (%ld only in bits 0:127 of zmm, %ld in mask registers)\n",
	       threads, seconds, c, m, l, k);
	return m != 0;
}
