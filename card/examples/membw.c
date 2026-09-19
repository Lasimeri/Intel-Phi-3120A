/*
 * membw.c: how much of the card's GDDR5 bandwidth is actually reachable.
 *
 * Intel ARK quotes 240 GB/s for the 3120A (12 channels). That is the
 * spec number; what matters for any memory-bound workload is what a
 * thread army can actually pull. Each thread owns a disjoint slice, so
 * there is no sharing and no false sharing, and the only thing being
 * measured is the path from GDDR5 to the cores.
 *
 * Scalar x87/integer loads are the floor here: without the VPU a single
 * thread cannot issue enough outstanding loads to saturate a channel,
 * which is exactly the question (can 228 slow threads do it instead).
 *
 *   cc -O2 -o membw membw.c -lpthread
 *   ./membw [MiB_TOTAL] [THREADS] [REPS]
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
#include <time.h>
#include <stdint.h>

static size_t g_bytes_per_thread;
static int g_reps;
static unsigned char *g_buf;
static pthread_barrier_t g_barrier;

static double now_s(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

/* Sum 8 bytes at a time with four independent accumulators, so the
 * in-order core has four loads in flight instead of a dependent chain. */
static void *reader(void *arg)
{
	long id = (long)arg;
	volatile uint64_t sink = 0;
	uint64_t *p = (uint64_t *)(g_buf + (size_t)id * g_bytes_per_thread);
	size_t n = g_bytes_per_thread / 8;
	int r;

	pthread_barrier_wait(&g_barrier);
	for (r = 0; r < g_reps; r++) {
		uint64_t a = 0, b = 0, c = 0, d = 0;
		size_t i;
		for (i = 0; i + 3 < n; i += 4) {
			a += p[i];
			b += p[i + 1];
			c += p[i + 2];
			d += p[i + 3];
		}
		sink += a + b + c + d;
	}
	(void)sink;
	return NULL;
}

int main(int argc, char **argv)
{
	size_t total_mib = (argc > 1) ? strtoul(argv[1], NULL, 10) : 2048;
	long threads = (argc > 2) ? strtol(argv[2], NULL, 10) : 228;
	double t0, t1, gb;
	pthread_t *tid;
	long i;

	g_reps = (argc > 3) ? atoi(argv[3]) : 3;
	g_bytes_per_thread = (total_mib << 20) / (size_t)threads;
	g_bytes_per_thread &= ~(size_t)63;      /* whole cache lines */

	g_buf = malloc(g_bytes_per_thread * (size_t)threads);
	if (!g_buf) {
		perror("malloc");
		return 1;
	}
	/* Touch everything first: page faults are not what we are timing. */
	memset(g_buf, 1, g_bytes_per_thread * (size_t)threads);

	tid = calloc((size_t)threads, sizeof(*tid));
	pthread_barrier_init(&g_barrier, NULL, (unsigned)threads + 1);
	for (i = 0; i < threads; i++)
		pthread_create(&tid[i], NULL, reader, (void *)i);

	pthread_barrier_wait(&g_barrier);
	t0 = now_s();
	for (i = 0; i < threads; i++)
		pthread_join(tid[i], NULL);
	t1 = now_s();

	gb = (double)g_bytes_per_thread * (double)threads * (double)g_reps / 1e9;
	printf("read %.2f GB with %ld threads in %.3f s = %.1f GB/s\n",
	       gb, threads, t1 - t0, gb / (t1 - t0));
	free(tid);
	free(g_buf);
	return 0;
}
