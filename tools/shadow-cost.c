/* What transparency costs on the host side.
 *
 * A host with no AVX-512 has no zmm registers, so a layer that executes
 * AVX-512 on its behalf must keep the program's 32 zmm values in memory, a
 * shadow register file, and do the arithmetic in the widest unit it does
 * have: two 256-bit AVX2 operations per 512-bit one.
 *
 * Three versions of the same degree-30 Horner evaluation:
 *   native   what the host would run if the program had used AVX2
 *   region   a whole region translated at once: live-in loaded from the
 *            shadow file, intermediates kept in ymm, live-out stored back
 *   insn     each AVX-512 instruction translated on its own: both sources
 *            read from the shadow file and the result written back, every
 *            time, because nothing carries between separately patched sites
 *
 * "insn" is what a naive per-instruction layer gets. "region" is what a
 * layer that translates whole loops gets. The gap between them is the
 * entire argument for region translation. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <immintrin.h>

#define DEG 30
#define LANES 16          /* float32 lanes in one AVX-512 register */

/* The shadow register file: 32 zmm registers, 64 bytes each. */
static __attribute__((aligned(64))) float zmm[32][LANES];
static float cv[DEG + 1];

static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec * 1e-9; }

/* ---- native: the host's own AVX2, eight lanes at a time ---- */
__attribute__((noinline)) static void poly_native(float *d, const float *x, long n)
{
    __m256 c[DEG + 1];
    for (int k = 0; k <= DEG; k++) c[k] = _mm256_set1_ps(cv[k]);
    for (long i = 0; i + 8 <= n; i += 8) {
        __m256 xv = _mm256_loadu_ps(x + i), acc = c[0];
        for (int k = 1; k <= DEG; k++) acc = _mm256_fmadd_ps(xv, acc, c[k]);
        _mm256_storeu_ps(d + i, acc);
    }
}

/* ---- region: one 512-bit lane group, intermediates stay in registers ---- */
/* ---- native, 16 lanes as two chains: the same instruction-level
 * parallelism the 512-bit-to-2x256 split produces, so the compare
 * isolates the shadow file rather than the unrolling. ---- */
__attribute__((noinline)) static void poly_native16(float *d, const float *x, long n)
{
    __m256 c[DEG + 1];
    for (int k = 0; k <= DEG; k++) c[k] = _mm256_set1_ps(cv[k]);
    for (long i = 0; i + 16 <= n; i += 16) {
        __m256 x0 = _mm256_loadu_ps(x + i), x1 = _mm256_loadu_ps(x + i + 8);
        __m256 a0 = c[0], a1 = c[0];
        for (int k = 1; k <= DEG; k++) {
            a0 = _mm256_fmadd_ps(x0, a0, c[k]);
            a1 = _mm256_fmadd_ps(x1, a1, c[k]);
        }
        _mm256_storeu_ps(d + i, a0);
        _mm256_storeu_ps(d + i + 8, a1);
    }
}

__attribute__((noinline)) static void poly_region(float *d, const float *x, long n)
{
    __m256 c[DEG + 1];
    for (int k = 0; k <= DEG; k++) c[k] = _mm256_set1_ps(cv[k]);
    for (long i = 0; i + LANES <= n; i += LANES) {
        /* live-in: read the 512-bit value from the shadow file as 2 x 256 */
        memcpy(zmm[1], x + i, LANES * 4);
        __m256 x0 = _mm256_load_ps(&zmm[1][0]), x1 = _mm256_load_ps(&zmm[1][8]);
        __m256 a0 = c[0], a1 = c[0];
        for (int k = 1; k <= DEG; k++) {
            a0 = _mm256_fmadd_ps(x0, a0, c[k]);
            a1 = _mm256_fmadd_ps(x1, a1, c[k]);
        }
        /* live-out: back to the shadow file, then to memory */
        _mm256_store_ps(&zmm[0][0], a0);
        _mm256_store_ps(&zmm[0][8], a1);
        memcpy(d + i, zmm[0], LANES * 4);
    }
}

/* ---- insn: every AVX-512 instruction goes through the shadow file ---- */
__attribute__((noinline)) static void poly_insn(float *d, const float *x, long n)
{
    for (long i = 0; i + LANES <= n; i += LANES) {
        memcpy(zmm[1], x + i, LANES * 4);
        for (int l = 0; l < LANES; l++) zmm[0][l] = cv[0];
        for (int k = 1; k <= DEG; k++) {
            for (int l = 0; l < LANES; l++) zmm[2][l] = cv[k];
            /* one vfmadd231ps: both sources from the shadow file, result
             * back to it, with a barrier so nothing is carried in a
             * register between what are separate translated sites. */
            __m256 s0 = _mm256_load_ps(&zmm[1][0]), s1 = _mm256_load_ps(&zmm[1][8]);
            __m256 a0 = _mm256_load_ps(&zmm[0][0]), a1 = _mm256_load_ps(&zmm[0][8]);
            __m256 b0 = _mm256_load_ps(&zmm[2][0]), b1 = _mm256_load_ps(&zmm[2][8]);
            _mm256_store_ps(&zmm[0][0], _mm256_fmadd_ps(s0, a0, b0));
            _mm256_store_ps(&zmm[0][8], _mm256_fmadd_ps(s1, a1, b1));
            __asm__ volatile("" ::: "memory");
        }
        memcpy(d + i, zmm[0], LANES * 4);
    }
}

int main(int argc, char **argv)
{
    long n = (argc > 1) ? atol(argv[1]) : (1L << 20);
    int reps = (argc > 2) ? atoi(argv[2]) : 20;
    n -= n % LANES;
    float *x = aligned_alloc(64, n * 4), *d = aligned_alloc(64, n * 4);
    for (int k = 0; k <= DEG; k++) cv[k] = 1.0f + (float)k * 0.01f;
    for (long i = 0; i < n; i++) x[i] = 0.5f + (float)(i % 64) * 0.001f;

    struct { const char *name; void (*fn)(float *, const float *, long); } v[] = {
        {"native AVX2 8-lane", poly_native}, {"native AVX2 16-lane", poly_native16}, {"region translated", poly_region}, {"per instruction", poly_insn},
    };
    double base = 0;
    printf("%-20s %10s %12s %10s\n", "", "seconds", "GFLOP/s", "vs native");
    for (unsigned k = 0; k < 4; k++) {
        v[k].fn(d, x, n);
        double t0 = now();
        for (int r = 0; r < reps; r++) v[k].fn(d, x, n);
        double dt = now() - t0;
        double g = 2.0 * DEG * n * reps / dt / 1e9;
        if (k == 1) base = g;
        printf("%-20s %10.3f %12.2f %9.2fx\n", v[k].name, dt, g, base / g);
    }
    return 0;
}
