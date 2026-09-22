/* Generate the test vectors and the bit-exact expected results for the
 * translated AVX-512 kernels in card/examples.
 *
 * Runs on the host, where fmaf() lowers to real FMA3 hardware, so every
 * expected file holds what AVX-512 hardware would have produced. The card
 * is then asked to reproduce those bits exactly. */
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <math.h>

#define N 4096

static uint32_t s = 0x12345678u;
static uint32_t rnd(void) { s ^= s << 13; s ^= s >> 17; s ^= s << 5; return s; }

static float bits(uint32_t u) { float f; memcpy(&f, &u, 4); return f; }

/* Four regions, so a failure says which class of value broke. */
static float pick(int i, int which)
{
    uint32_t r = rnd();
    if (i < 1024) {
        /* ordinary: exponents near 1.0 */
        return bits((r & 0x807fffffu) | (0x3f000000u + ((r & 7) << 23)));
    } else if (i < 2048) {
        /* wide dynamic range, so the fma has something to round */
        return bits((r & 0x807fffffu) | ((0x20u + (r % 0xc0u)) << 23));
    } else if (i < 3072) {
        /* denormals and the smallest normals */
        return bits((r & 0x807fffffu) | ((which == 2) ? 0u : 0u));
    } else {
        static const uint32_t sp[] = {
            0x7f800000u, 0xff800000u, 0x7fc00001u, 0x00000000u, 0x80000000u,
            0x00000001u, 0x007fffffu, 0x00800000u, 0x7f7fffffu, 0x3f800000u,
        };
        return bits(sp[r % (sizeof sp / sizeof sp[0])]);
    }
}

static int gen_fma(void)
{
    static float a[N], b[N], c[N], d[N];
    for (int i = 0; i < N; i++) { a[i] = pick(i, 0); b[i] = pick(i, 1); c[i] = pick(i, 2); }
    for (int i = 0; i < N; i++) d[i] = fmaf(a[i], b[i], c[i]);

    struct { const char *n; const void *p; } f[] = {
        {"a.bin", a}, {"b.bin", b}, {"c.bin", c}, {"expected.bin", d},
    };
    for (unsigned k = 0; k < 4; k++) {
        FILE *fp = fopen(f[k].n, "wb");
        if (!fp) { perror(f[k].n); return 1; }
        if (fwrite(f[k].p, 4, N, fp) != N) { perror("write"); return 1; }
        fclose(fp);
    }
    printf("wrote %d floats per file\n", N);
    printf("region 0 ordinary, 1 wide range, 2 denormal, 3 specials\n");
    return 0;
}

#define PN 65536
#define DEG 30
#define LANES 16


static int gen_poly(void)
{
    static float x[PN], d[PN];
    static float coef[(DEG + 1) * LANES];
    float cv[DEG + 1];

    for (int k = 0; k <= DEG; k++) {
        cv[k] = bits((rnd() & 0x807fffffu) | (0x3d800000u + ((rnd() & 3) << 23)));
        for (int l = 0; l < LANES; l++) coef[k * LANES + l] = cv[k];
    }
    for (int i = 0; i < PN; i++) x[i] = bits((rnd() & 0x807fffffu) | 0x3f000000u);

    for (int i = 0; i < PN; i++) {
        float acc = cv[0];
        for (int k = 1; k <= DEG; k++) acc = fmaf(x[i], acc, cv[k]);
        d[i] = acc;
    }

    struct { const char *n; const void *p; size_t c; } f[] = {
        {"x.bin", x, PN}, {"coef.bin", coef, (DEG + 1) * LANES}, {"poly_expected.bin", d, PN},
    };
    for (unsigned k = 0; k < 3; k++) {
        FILE *fp = fopen(f[k].n, "wb");
        if (!fp) { perror(f[k].n); return 1; }
        fwrite(f[k].p, 4, f[k].c, fp);
        fclose(fp);
    }
    printf("wrote %d inputs, %d coefficients\n", PN, DEG + 1);
    return 0;
}

int main(void)
{
    if (gen_fma()) return 1;
    if (gen_poly()) return 1;
    return 0;
}
