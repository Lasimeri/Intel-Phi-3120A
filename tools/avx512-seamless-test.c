/* avx512-seamless-test: an ordinary program that uses AVX-512 through
 * intrinsics and checks its own answers. Nothing in it knows about the
 * card, the emulator, the wrapper or this host's lack of AVX-512; it is
 * the program a user would have. Compiled with -mavx512f it dies with
 * SIGILL on the 5800X; under scripts/phi512.sh every AVX-512 instruction
 * is intercepted and performed on the program's behalf. The program
 * reports which lanes are right, how long the AVX-512 section took, and
 * where it ran according to the only thing it can see: /proc/cpuinfo.
 *
 * Three kernels, each 16 floats wide, each checked against a scalar
 * reference compiled with AVX-512 forbidden (target("no-avx512f")), so
 * the reference cannot be intercepted too:
 *   1. a degree-30 polynomial by Horner's rule (31 fmadd per vector),
 *      the same shape the card's compiled-in kernel has;
 *   2. a dot product with a reduce at the end (fmadd, add, reduce);
 *   3. an integer lane update with a mask (vpaddd, vpcmpd, masked move).
 *
 *   gcc -O2 -mavx512f -mno-avx512vl -mno-avx512bw -mno-avx512dq -mno-avx512cd \
 *       -o avx512-seamless-test tools/avx512-seamless-test.c -lm
 *   ./avx512-seamless-test [N]         N elements, a multiple of 16 (default 65536)
 *
 * See avx512-seamless-test.md. */
#include <immintrin.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define DEG 30
#define LANES 16

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec / 1e9;
}

static int host_has_avx512(void)
{
    FILE *f = fopen("/proc/cpuinfo", "r");
    char line[4096];
    int has = 0;
    if (!f) return -1;
    while (fgets(line, sizeof line, f))
        if (!strncmp(line, "flags", 5) && strstr(line, " avx512f")) { has = 1; break; }
    fclose(f);
    return has;
}

/* ---- the AVX-512 kernels: what a program compiled for AVX-512 does ---- */

static void poly_avx512(float *out, const float *x, const float *coef, long n)
{
    for (long i = 0; i < n; i += LANES) {
        __m512 v = _mm512_loadu_ps(x + i);
        __m512 acc = _mm512_set1_ps(coef[DEG]);
        for (int k = DEG - 1; k >= 0; k--)
            acc = _mm512_fmadd_ps(acc, v, _mm512_set1_ps(coef[k]));
        _mm512_storeu_ps(out + i, acc);
    }
}

static float dot_avx512(const float *a, const float *b, long n)
{
    __m512 acc = _mm512_setzero_ps();
    for (long i = 0; i < n; i += LANES)
        acc = _mm512_fmadd_ps(_mm512_loadu_ps(a + i), _mm512_loadu_ps(b + i), acc);
    return _mm512_reduce_add_ps(acc);
}

/* out[i] = in[i] + 7 where in[i] is even, in[i] * 3 where odd: a compare
 * into a mask register and two masked operations. */
static void ints_avx512(int32_t *out, const int32_t *in, long n)
{
    const __m512i one = _mm512_set1_epi32(1), seven = _mm512_set1_epi32(7), three = _mm512_set1_epi32(3);
    for (long i = 0; i < n; i += LANES) {
        __m512i v = _mm512_loadu_si512((const void *)(in + i));
        __mmask16 odd = _mm512_cmpeq_epi32_mask(_mm512_and_si512(v, one), one);
        __m512i r = _mm512_add_epi32(v, seven);
        r = _mm512_mask_mullo_epi32(r, odd, v, three);
        _mm512_storeu_si512((void *)(out + i), r);
    }
}

/* ---- scalar references, AVX-512 forbidden so they run natively ---- */

__attribute__((target("no-avx512f"), noinline))
static void poly_ref(float *out, const float *x, const float *coef, long n)
{
    for (long i = 0; i < n; i++) {
        float acc = coef[DEG];
        for (int k = DEG - 1; k >= 0; k--)
            acc = fmaf(acc, x[i], coef[k]);
        out[i] = acc;
    }
}

__attribute__((target("no-avx512f"), noinline))
static float dot_ref(const float *a, const float *b, long n)
{
    /* Sixteen partial sums in lane order, then the same tree reduce the
     * intrinsic performs, so the rounding order matches. */
    float lane[LANES] = { 0 };
    for (long i = 0; i < n; i += LANES)
        for (int l = 0; l < LANES; l++)
            lane[l] = fmaf(a[i + l], b[i + l], lane[l]);
    for (int w = LANES / 2; w >= 1; w /= 2)
        for (int l = 0; l < w; l++)
            lane[l] += lane[l + w];
    return lane[0];
}

__attribute__((target("no-avx512f"), noinline))
static void ints_ref(int32_t *out, const int32_t *in, long n)
{
    for (long i = 0; i < n; i++)
        out[i] = (in[i] & 1) ? in[i] * 3 : in[i] + 7;
}

static long count_bad(const void *a, const void *b, long n)
{
    const uint32_t *p = a, *q = b;
    long bad = 0;
    for (long i = 0; i < n; i++) bad += p[i] != q[i];
    return bad;
}

int main(int argc, char **argv)
{
    long n = argc > 1 ? atol(argv[1]) : 65536;
    if (n < LANES || n % LANES) { fprintf(stderr, "N must be a positive multiple of %d\n", LANES); return 2; }

    float *x, *a, *b, *coef, *out, *ref;
    int32_t *iin, *iout, *iref;
    if (posix_memalign((void **)&x, 64, n * 4) || posix_memalign((void **)&a, 64, n * 4) ||
        posix_memalign((void **)&b, 64, n * 4) || posix_memalign((void **)&out, 64, n * 4) ||
        posix_memalign((void **)&ref, 64, n * 4) || posix_memalign((void **)&iin, 64, n * 4) ||
        posix_memalign((void **)&iout, 64, n * 4) || posix_memalign((void **)&iref, 64, n * 4) ||
        posix_memalign((void **)&coef, 64, (DEG + 1) * 4))
        return 1;
    uint64_t s = 0x9e3779b97f4a7c15ULL;
    for (long i = 0; i < n; i++) {
        s ^= s << 13; s ^= s >> 7; s ^= s << 17;
        x[i] = (float)((s >> 11) & 0xffff) / 65536.0f * 2.0f - 1.0f;   /* [-1, 1): Horner stays finite */
        a[i] = (float)((s >> 27) & 0xffff) / 32768.0f - 1.0f;
        b[i] = (float)((s >> 43) & 0xffff) / 32768.0f - 1.0f;
        iin[i] = (int32_t)(s >> 33);
    }
    for (int k = 0; k <= DEG; k++) coef[k] = 1.0f / (float)(k + 1) * ((k & 1) ? -1.0f : 1.0f);

    int has = host_has_avx512();
    printf("host: /proc/cpuinfo %s avx512f\n", has == 1 ? "lists" : has == 0 ? "does NOT list" : "unreadable for");
    printf("elements: %ld, AVX-512 vectors per kernel: %ld\n", n, n / LANES);

    double t0 = now();
    poly_avx512(out, x, coef, n);
    double t1 = now();
    float dot = dot_avx512(a, b, n);
    double t2 = now();
    ints_avx512(iout, iin, n);
    double t3 = now();

    poly_ref(ref, x, coef, n);
    float dref = dot_ref(a, b, n);
    ints_ref(iref, iin, n);

    long bad_poly = count_bad(out, ref, n), bad_ints = count_bad(iout, iref, n);
    int bad_dot = memcmp(&dot, &dref, 4) != 0;
    printf("polynomial (31 fmadd per vector):  %8.3f ms  %s\n", (t1 - t0) * 1e3,
           bad_poly ? "MISMATCH" : "every lane bit-identical to the scalar reference");
    if (bad_poly) printf("  %ld of %ld lanes differ\n", bad_poly, n);
    printf("dot product (fmadd + reduce):      %8.3f ms  %s (%.9g)\n", (t2 - t1) * 1e3,
           bad_dot ? "MISMATCH" : "bit-identical", (double)dot);
    if (bad_dot) printf("  got %.9g want %.9g\n", (double)dot, (double)dref);
    printf("integers (compare, mask, mullo):   %8.3f ms  %s\n", (t3 - t2) * 1e3,
           bad_ints ? "MISMATCH" : "every lane identical");
    if (bad_ints) printf("  %ld of %ld lanes differ\n", bad_ints, n);
    int ok = !bad_poly && !bad_dot && !bad_ints;
    printf("%s\n", ok ? "PASS: the AVX-512 code ran and its answers are right" : "FAIL");
    return ok ? 0 : 1;
}
