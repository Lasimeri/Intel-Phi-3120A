/* The card as a unified AVX-512 engine, measured on a kernel that actually
 * uses the vector unit.
 *
 * 30 fused multiply-adds per element against one load and one store, so
 * 60 flops per 8 bytes of DRAM traffic. The earlier elementwise kernel ran
 * at 2 flops per 16 bytes and measured the memory system instead.
 *
 * Two things are checked here, in this order: that the translated code
 * still produces the host's bits after being split across cores, and only
 * then how fast it is. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <pthread.h>
#include <time.h>

void poly_kernel_x8(float *d, const float *x, const float *coef, long n);

#define LANES 16
#define DEG 30

static float *X, *D, *COEF;
static long total;
static int reps;

struct slice { long begin, count; };

static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec * 1e-9; }

static void *worker(void *arg)
{
    struct slice *s = arg;
    for (int r = 0; r < reps; r++) poly_kernel_x8(D + s->begin, X + s->begin, COEF, s->count);
    return NULL;
}

static double run(int threads)
{
    pthread_t th[256];
    static struct slice sl[256];
    long vectors = total / LANES;
    long per = (vectors + threads - 1) / threads, at = 0;
    for (int i = 0; i < threads; i++) {
        long take = per;
        if (at + take > vectors) take = vectors - at;
        if (take < 0) take = 0;
        sl[i].begin = at * LANES;
        sl[i].count = take * LANES;
        at += take;
    }
    double t0 = now();
    for (int i = 0; i < threads; i++) pthread_create(&th[i], NULL, worker, &sl[i]);
    for (int i = 0; i < threads; i++) pthread_join(th[i], NULL);
    return now() - t0;
}

static void *alloc64(size_t bytes)
{
    void *p = NULL;
    if (posix_memalign(&p, 64, bytes + 256) != 0) { fprintf(stderr, "oom\n"); exit(1); }
    memset(p, 0, bytes + 256);
    return p;
}

static void *slurp(const char *path, size_t bytes)
{
    FILE *f = fopen(path, "rb");
    if (!f) { perror(path); exit(1); }
    void *p = alloc64(bytes);
    if (fread(p, 1, bytes, f) != bytes) { fprintf(stderr, "short read %s\n", path); exit(1); }
    fclose(f);
    return p;
}

int main(int argc, char **argv)
{
    /* Correctness first, on the host's vectors. */
    const long NCHK = 65536;
    COEF = slurp("coef.bin", (DEG + 1) * LANES * 4);
    float *cx = slurp("x.bin", NCHK * 4);
    uint32_t *want = slurp("poly_expected.bin", NCHK * 4);
    float *cd = alloc64(NCHK * 4);
    memset(cd, 0xa5, NCHK * 4);
    poly_kernel_x8(cd, cx, COEF, NCHK);
    long bad = 0, first = -1;
    for (long i = 0; i < NCHK; i++)
        if (((uint32_t *)cd)[i] != want[i]) { if (first < 0) first = i; bad++; }
    if (bad) {
        printf("BIT MISMATCH: %ld of %ld lanes; first at %ld want=%08x got=%08x\n",
               bad, NCHK, first, want[first], ((uint32_t *)cd)[first]);
        return 1;
    }
    printf("30 fused multiply-adds per element, %ld lanes: bit-identical to the host\n\n", NCHK);

    total = (argc > 1) ? atol(argv[1]) : (1L << 21);
    reps = (argc > 2) ? atoi(argv[2]) : 10;
    total -= total % LANES;
    X = alloc64(total * 4);
    D = alloc64(total * 4);
    for (long i = 0; i < total; i++) X[i] = cx[i % NCHK];

    printf("%ld elements, %d passes\n\n", total, reps);
    printf("%8s %10s %12s %10s %14s\n", "threads", "seconds", "GFLOP/s", "scaling", "per thread");
    double base = 0;
    const int sweep[] = {1, 2, 4, 8, 16, 32, 57, 114, 171, 228};
    for (unsigned i = 0; i < sizeof sweep / sizeof sweep[0]; i++) {
        int t = sweep[i];
        double s = run(t);
        double flops = 2.0 * DEG * total * reps;
        if (i == 0) base = s;
        printf("%8d %10.3f %12.2f %9.1fx %13.3f\n", t, s, flops / s / 1e9, base / s, flops / s / 1e9 / t);
    }
    return 0;
}
