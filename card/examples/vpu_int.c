/* vpu_int.c: does the card execute the integer MVEX encodings the way the
 * ISA document says? One instruction per output slot, every lane compared
 * against a scalar model written here.
 *
 * The float64 encodings in host/crates/knc-mvex are pinned to Intel's k1om
 * kernel macros, so a mistake there shows up as a unit-test failure on the
 * host. The integer encodings have no such reference: they come from the
 * opcode table in ISA reference 327364-001 section 6 and nothing else. A
 * wrong prefix bit rarely faults, it decodes as a different valid
 * instruction and returns plausible numbers, so the only real check is
 * semantic and it has to run here.
 *
 * Build on the card:  cc -O2 -o vpu_int vpu_int.c vpu_int.S
 * See vpu_int.md.
 */
#include <stdio.h>
#include <stdlib.h>

#define LANES 16
#define SLOTS 16

void vpu_int_probe(const int *in, int *out);

/* Arithmetic right shift without relying on the implementation-defined
 * behaviour of >> on a negative signed int. */
static int sar(int v, unsigned c)
{
	unsigned u = (unsigned)v;
	return v < 0 ? (int)~(~u >> c) : (int)(u >> c);
}

/* The card sets a lane to zero when the variable shift count exceeds 31
 * (ISA reference, VPSLLVD and VPSRLVD, "Description"). */
static int shlv(int v, unsigned c)
{
	return c > 31 ? 0 : (int)((unsigned)v << c);
}

static int shrv(int v, unsigned c)
{
	return c > 31 ? 0 : (int)((unsigned)v >> c);
}

static const char *NAME[SLOTS] = {
	"vpaddd  zmm2, zmm0, zmm1",
	"vpsubd  zmm2, zmm0, zmm1",
	"vpandd  zmm2, zmm0, zmm1",
	"vpandnd zmm2, zmm0, zmm1",
	"vpord   zmm2, zmm0, zmm1",
	"vpxord  zmm2, zmm0, zmm1",
	"vpslld  zmm2, zmm0, 1",
	"vpslld  zmm2, zmm0, 11",
	"vpsrld  zmm2, zmm0, 11",
	"vpsrad  zmm2, zmm0, 11",
	"vpsrad  zmm2, zmm0, 31",
	"vpslld  zmm2, [rdi], 11   (memory source)",
	"vpsllvd zmm2, zmm0, zmm1",
	"vpsrlvd zmm2, zmm0, [rdi+64]",
	"vpaddd  zmm2 {k1}, zmm0, zmm1   (merge mask 0x00ff)",
	"vpslld  zmm17 {k2}, zmm1, 3     (merge mask 0x0f0f, V' bit)",
};

static void reference(const int *a, const int *b, int ref[SLOTS][LANES])
{
	int i;

	for (i = 0; i < LANES; i++) {
		unsigned ua = (unsigned)a[i], ub = (unsigned)b[i];
		unsigned c = (unsigned)b[i];

		ref[0][i] = (int)(ua + ub);
		ref[1][i] = (int)(ua - ub);
		ref[2][i] = (int)(ua & ub);
		ref[3][i] = (int)(~ua & ub);
		ref[4][i] = (int)(ua | ub);
		ref[5][i] = (int)(ua ^ ub);
		ref[6][i] = (int)(ua << 1);
		ref[7][i] = (int)(ua << 11);
		ref[8][i] = (int)(ua >> 11);
		ref[9][i] = sar(a[i], 11);
		ref[10][i] = sar(a[i], 31);
		ref[11][i] = (int)(ua << 11);
		ref[12][i] = shlv(a[i], c);
		ref[13][i] = shrv(a[i], c);
		ref[14][i] = (0x00ff >> i) & 1 ? (int)(ua + ub) : a[i];
		ref[15][i] = (0x0f0f >> i) & 1 ? (int)(ub << 3) : a[i];
	}
}

/* Returns the number of failing slots. */
static int run(const char *what, const int *a, const int *b, int *in, int *out)
{
	int ref[SLOTS][LANES];
	int s, i, failed = 0;

	for (i = 0; i < LANES; i++) { in[i] = a[i]; in[LANES + i] = b[i]; }
	for (s = 0; s < SLOTS * LANES; s++) out[s] = (int)0xDEADBEEF;

	vpu_int_probe(in, out);
	reference(a, b, ref);

	printf("\n%s\n", what);
	for (s = 0; s < SLOTS; s++) {
		int bad = -1;

		for (i = 0; i < LANES; i++)
			if (out[s * LANES + i] != ref[s][i]) { bad = i; break; }
		if (bad < 0) {
			printf("  %-2d OK     %s\n", s, NAME[s]);
		} else {
			failed++;
			printf("  %-2d FAILED %s\n", s, NAME[s]);
			printf("       lane %d: a=%08x b=%08x got=%08x want=%08x\n", bad,
			       (unsigned)a[bad], (unsigned)b[bad],
			       (unsigned)out[s * LANES + bad], (unsigned)ref[s][bad]);
		}
	}
	return failed;
}

int main(void)
{
	int *in, *out;
	int a[LANES], b1[LANES], b2[LANES];
	int i, failed = 0;

	if (posix_memalign((void **)&in, 64, 2 * LANES * sizeof *in)
	    || posix_memalign((void **)&out, 64, SLOTS * LANES * sizeof *out)) {
		perror("posix_memalign");
		return 1;
	}

	/* Negative and positive lanes, bits set high and low. */
	for (i = 0; i < LANES; i++)
		a[i] = (int)(0x9E3779B9u * (unsigned)(i + 1) ^ ((unsigned)i << 20));

	/* Pass 1 exercises the shifts: counts spanning 0 to 33, so the
	 * "greater than 31 gives zero" rule is covered on both variable
	 * shifts. Non-negative, because the document does not say what a
	 * negative count does and a test should not guess. */
	{
		static const int counts[LANES] = { 0, 1, 2, 4, 8, 16, 31, 32, 33, 3, 5, 7, 11, 17, 23, 30 };
		for (i = 0; i < LANES; i++) b1[i] = counts[i];
	}

	/* Pass 2 exercises the bitwise operators, which pass 1 barely does
	 * because its second operand has only six significant bits. Every
	 * variable shift count here is well above 31, so those lanes go to
	 * zero, which is the other half of the same rule. */
	for (i = 0; i < LANES; i++)
		b2[i] = (int)((0xB5297A4Du * (unsigned)(i + 1) ^ 0x68E31DA4u) & 0x7FFFFFFFu);

	failed += run("pass 1: second operand is a shift count (0 to 33)", a, b1, in, out);
	failed += run("pass 2: second operand is a full bit pattern", a, b2, in, out);

	printf("\n%d of %d checks failed\n", failed, 2 * SLOTS);
	free(in);
	free(out);
	return failed ? 1 : 0;
}
