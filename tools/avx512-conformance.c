/* Narrow down which integer operation the emulator gets wrong, by doing
 * each one on its own over the same data. */
#include <stdio.h>
#define N 1024
static int x[N], y[N];

#define KERNEL(name, expr) \
    __attribute__((noinline)) static long name(void) { long s = 0; \
        for (int i = 0; i < N; i++) s += (expr); return s; }

KERNEL(k_and,   x[i] & y[i])
KERNEL(k_or,    x[i] | y[i])
KERNEL(k_xor,   x[i] ^ y[i])
KERNEL(k_add,   x[i] + y[i])
KERNEL(k_sub,   x[i] - y[i])
KERNEL(k_mul,   x[i] * y[i])
KERNEL(k_shl,   x[i] << 3)
KERNEL(k_shr,   x[i] >> 3)
KERNEL(k_ushr,  (int)((unsigned)x[i] >> 3))
KERNEL(k_sel,   (x[i] & 0x10) ? y[i] : -y[i])
KERNEL(k_cmp,   (x[i] > y[i]) ? 1 : 0)
KERNEL(k_max,   x[i] > y[i] ? x[i] : y[i])

int main(void)
{
    for (int i = 0; i < N; i++) { x[i] = i * 2654435761u; y[i] = i * 40503 + 7; }
    struct { const char *n; long (*f)(void); } t[] = {
        {"and", k_and}, {"or", k_or}, {"xor", k_xor}, {"add", k_add}, {"sub", k_sub},
        {"mul", k_mul}, {"shl", k_shl}, {"shr", k_shr}, {"ushr", k_ushr},
        {"sel", k_sel}, {"cmp", k_cmp}, {"max", k_max},
    };
    for (unsigned i = 0; i < sizeof t / sizeof t[0]; i++) printf("%-6s %ld\n", t[i].n, t[i].f());
    return 0;
}
