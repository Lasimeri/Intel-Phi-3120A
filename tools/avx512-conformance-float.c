/* Ordinary C, compiled with -mavx512f. Nothing here is hand-written
 * assembly: this is whatever the compiler decides to emit, which is the
 * honest test of how much of AVX-512 a program actually uses. */
#include <stdio.h>
#include <stdlib.h>
#include <math.h>

#define N 4096

__attribute__((noinline)) static float conditional_sum(const float *a, const float *b, int n)
{
    float s = 0;
    for (int i = 0; i < n; i++) s += (a[i] > b[i]) ? a[i] * 2.0f : b[i] / 3.0f;
    return s;
}

__attribute__((noinline)) static int count_and_mask(const int *x, int n)
{
    int c = 0;
    for (int i = 0; i < n; i++) c += (x[i] & 0x10) ? (x[i] >> 2) : -(x[i] | 7);
    return c;
}

__attribute__((noinline)) static double widen_and_scale(const float *a, int n)
{
    double s = 0;
    for (int i = 0; i < n; i++) s += (double)a[i] * 1.5 - 0.25;
    return s;
}

int main(void)
{
    static float a[N], b[N];
    static int x[N];
    for (int i = 0; i < N; i++) {
        a[i] = (float)((i * 37) % 101) * 0.5f;
        b[i] = (float)((i * 53) % 97) * 0.25f;
        x[i] = i * 2654435761u;
    }
    printf("conditional_sum = %.6f\n", conditional_sum(a, b, N));
    printf("count_and_mask  = %d\n", count_and_mask(x, N));
    printf("widen_and_scale = %.6f\n", widen_and_scale(a, N));
    return 0;
}
