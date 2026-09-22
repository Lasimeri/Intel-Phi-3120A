/* An ordinary program that uses AVX-512. Nothing in it knows about the
 * card, the emulator, or this host's lack of AVX-512. It is compiled
 * normally and linked against an AVX-512 assembly kernel. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#define N 65536
#define DEG 30
#define LANES 16

void poly_kernel_x8(float *d, const float *x, const float *coef, long n);

static void *slurp(const char *p, size_t bytes)
{
    FILE *f = fopen(p, "rb");
    if (!f) { perror(p); exit(1); }
    void *q = NULL;
    if (posix_memalign(&q, 64, bytes + 256)) exit(1);
    memset(q, 0, bytes + 256);
    if (fread(q, 1, bytes, f) != bytes) { fprintf(stderr, "short read %s\n", p); exit(1); }
    fclose(f);
    return q;
}

int main(void)
{
    float *x = slurp("x.bin", N * 4);
    float *coef = slurp("coef.bin", (DEG + 1) * LANES * 4);
    uint32_t *want = slurp("poly_expected.bin", N * 4);
    float *d = NULL;
    if (posix_memalign((void **)&d, 64, N * 4 + 256)) return 1;
    memset(d, 0xa5, N * 4);

    printf("calling an AVX-512 kernel on a CPU with no AVX-512...\n");
    fflush(stdout);
    poly_kernel_x8(d, x, coef, N);

    long bad = 0, first = -1;
    for (long i = 0; i < N; i++)
        if (((uint32_t *)d)[i] != want[i]) { if (first < 0) first = i; bad++; }
    if (bad) {
        printf("WRONG: %ld of %d lanes differ; first at %ld want=%08x got=%08x\n",
               bad, N, first, want[first], ((uint32_t *)d)[first]);
        return 1;
    }
    printf("all %d lanes bit-identical to AVX-512 hardware\n", N);
    return 0;
}
