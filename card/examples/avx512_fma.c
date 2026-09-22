/* fma_check: run the translated AVX-512 kernel on the card and compare its
 * output, bit for bit, against what the host's FMA3 hardware produced.
 *
 * The kernel came out of avx512-xlate; the expected file came from fmaf()
 * on the host. Agreement to the bit is the claim being tested.
 *
 * Buffers carry 64 bytes of slack past the end because the card's unaligned
 * access pair always touches both cache lines an access could straddle,
 * which for the final iteration is one line past the data. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#define N 4096
#define SLACK 64

void fma_kernel(float *d, const float *a, const float *b, const float *c, long n);

static void *slurp(const char *path, size_t bytes)
{
    FILE *f = fopen(path, "rb");
    if (!f) { perror(path); exit(1); }
    void *p = malloc(bytes);
    if (!p || fread(p, 1, bytes, f) != bytes) { fprintf(stderr, "short read %s\n", path); exit(1); }
    fclose(f);
    return p;
}

static const char *region_name(int i)
{
    if (i < 1024) return "ordinary";
    if (i < 2048) return "wide range";
    if (i < 3072) return "denormal";
    return "specials";
}

int main(void)
{
    float *a = slurp("a.bin", N * 4);
    float *b = slurp("b.bin", N * 4);
    float *c = slurp("c.bin", N * 4);
    uint32_t *want = slurp("expected.bin", N * 4);

    /* One allocation big enough to place the working copies at any of the
     * byte offsets below, each with slack behind it. */
    const int offsets[] = {0, 4, 32, 60};
    int failures = 0;

    for (unsigned o = 0; o < sizeof offsets / sizeof offsets[0]; o++) {
        int off = offsets[o];
        char *base = malloc(4 * (N * 4 + 64) + 256);
        if (!base) { fprintf(stderr, "oom\n"); return 1; }
        memset(base, 0, 4 * (N * 4 + 64) + 256);

        size_t stride = N * 4 + SLACK + 64;
        float *A = (float *)(base + off);
        float *B = (float *)(base + off + stride);
        float *C = (float *)(base + off + 2 * stride);
        float *D = (float *)(base + off + 3 * stride);
        memcpy(A, a, N * 4);
        memcpy(B, b, N * 4);
        memcpy(C, c, N * 4);
        memset(D, 0xa5, N * 4);

        fma_kernel(D, A, B, C, N);

        uint32_t *got = (uint32_t *)D;
        int bad = 0, first = -1;
        int per_region[4] = {0, 0, 0, 0};
        for (int i = 0; i < N; i++) {
            if (got[i] != want[i]) {
                /* Two NaNs with different payloads are still both NaN, but
                 * this test demands the bits, so no exception is made. */
                bad++;
                per_region[i / 1024]++;
                if (first < 0) first = i;
            }
        }
        if (bad == 0) {
            printf("offset %2d: all %d lanes bit-identical\n", off, N);
        } else {
            failures++;
            printf("offset %2d: %d of %d lanes differ\n", off, bad, N);
            for (int r = 0; r < 4; r++)
                if (per_region[r]) printf("             region %d (%s): %d\n", r, region_name(r * 1024), per_region[r]);
            printf("             first at %d (%s): a=%08x b=%08x c=%08x want=%08x got=%08x\n",
                   first, region_name(first), ((uint32_t *)a)[first], ((uint32_t *)b)[first],
                   ((uint32_t *)c)[first], want[first], got[first]);
        }
        free(base);
    }
    printf("%s\n", failures ? "MISMATCH" : "bit-exact at every alignment");
    return failures != 0;
}
