/* bitunpack.c: is hand-written MVEX bit-unpacking worth it on this card?
 *
 * Three implementations of the same thing, unpacking 1024 values of N bits
 * into 1024 int32:
 *
 *   stream   the ordinary way a codec does it: one contiguous bitstream,
 *            scalar, one value at a time. This is the baseline that matters,
 *            because it is what the card runs today.
 *   lanes    the same work on the lane-interleaved layout, still scalar.
 *            This is the FastLanes scalar path, and it isolates how much of
 *            any win comes from the layout rather than from the vector unit.
 *   vpu      the lane-interleaved layout through knc_unpack_bN, 16 values
 *            per shift-and-mask pair (bitunpack.S, generated).
 *
 * Both a hot number (one block, resident in L1) and a streaming number
 * (16 MiB of output, past every cache) are printed, because the VPU memcpy
 * work showed the card runs out of bandwidth before it runs out of vector
 * throughput (docs/results/2026-09-20-zstd-vpu.md).
 *
 * Build on the card:  cc -O2 -o bitunpack bitunpack.c bitunpack.S
 * See bitunpack.md.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define BLOCK 1024
#define LANES 16

void knc_unpack_b5(int *out, const unsigned *packed);
void knc_unpack_b11(int *out, const unsigned *packed);
void knc_unpack_b16(int *out, const unsigned *packed);

static const unsigned WIDTHS[] = { 5, 11, 16 };
#define NWIDTHS (sizeof WIDTHS / sizeof WIDTHS[0])

static void (*const KERNEL[NWIDTHS])(int *, const unsigned *) = {
	knc_unpack_b5, knc_unpack_b11, knc_unpack_b16
};

static double now_s(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

static unsigned lowmask(unsigned bits)
{
	return bits >= 32 ? 0xFFFFFFFFu : (1u << bits) - 1u;
}

/* Words per block in either layout: 1024 values of `bits` bits. */
static size_t words(unsigned bits)
{
	return (size_t)BLOCK * bits / 32;
}

/* Lane-interleaved: value i is lane i%16 at position i/16, and word w of
 * every lane is one 64-byte vector at packed + 16*w. */
static void pack_lanes(unsigned *p, const int *v, unsigned bits)
{
	unsigned m = lowmask(bits);
	int i;

	memset(p, 0, words(bits) * sizeof *p);
	for (i = 0; i < BLOCK; i++) {
		unsigned lane = (unsigned)i % LANES, pos = (unsigned)i / LANES;
		unsigned bit = pos * bits, w = bit / 32, s = bit % 32;
		unsigned val = (unsigned)v[i] & m;

		p[(size_t)w * LANES + lane] |= val << s;
		if (s + bits > 32)
			p[(size_t)(w + 1) * LANES + lane] |= val >> (32 - s);
	}
}

/* Written the way FastLanes writes it: position outside, lane inside, so
 * the shift counts are loop invariants and the inner loop is sixteen
 * identical operations. That shape is the whole point of the layout, and
 * writing it the obvious way instead (one loop over values, deriving lane
 * and position with a divide and a modulo) costs a factor of 2.5 and
 * measures the traversal rather than the layout. */
static void unpack_lanes(int *out, const unsigned *p, unsigned bits)
{
	unsigned m = lowmask(bits);
	int pos, lane;

	for (pos = 0; pos < BLOCK / LANES; pos++) {
		unsigned bit = (unsigned)pos * bits, w = bit / 32, s = bit % 32;

		if (s + bits > 32) {
			for (lane = 0; lane < LANES; lane++)
				out[pos * LANES + lane] = (int)(((p[(size_t)w * LANES + lane] >> s)
					| (p[(size_t)(w + 1) * LANES + lane] << (32 - s))) & m);
		} else {
			for (lane = 0; lane < LANES; lane++)
				out[pos * LANES + lane] = (int)((p[(size_t)w * LANES + lane] >> s) & m);
		}
	}
}

/* One contiguous bitstream, the ordinary layout. */
static void pack_stream(unsigned *p, const int *v, unsigned bits)
{
	unsigned m = lowmask(bits);
	int i;

	memset(p, 0, words(bits) * sizeof *p);
	for (i = 0; i < BLOCK; i++) {
		unsigned bit = (unsigned)i * bits, w = bit / 32, s = bit % 32;
		unsigned val = (unsigned)v[i] & m;

		p[w] |= val << s;
		if (s + bits > 32)
			p[w + 1] |= val >> (32 - s);
	}
}

static void unpack_stream(int *out, const unsigned *p, unsigned bits)
{
	unsigned m = lowmask(bits);
	int i;

	for (i = 0; i < BLOCK; i++) {
		unsigned bit = (unsigned)i * bits, w = bit / 32, s = bit % 32;
		unsigned val = p[w] >> s;

		if (s + bits > 32)
			val |= p[w + 1] << (32 - s);
		out[i] = (int)(val & m);
	}
}

int main(int argc, char **argv)
{
	/* 4096 blocks of 1024 int32 is 16 MiB of output, well past the 512 KiB
	 * of L2 a core has, so the streaming pass measures memory and the hot
	 * pass measures instructions. */
	size_t blocks = (argc > 1) ? (size_t)strtoul(argv[1], NULL, 10) : 4096;
	int *values, *out, *ref;
	unsigned *lanes_buf, *stream_buf;
	size_t k, b;
	int i, bad = 0;

	if (posix_memalign((void **)&values, 64, BLOCK * sizeof *values)
	    || posix_memalign((void **)&ref, 64, BLOCK * sizeof *ref)
	    || posix_memalign((void **)&out, 64, blocks * BLOCK * sizeof *out)
	    || posix_memalign((void **)&lanes_buf, 64, blocks * words(32) * sizeof *lanes_buf)
	    || posix_memalign((void **)&stream_buf, 64, blocks * words(32) * sizeof *stream_buf)) {
		perror("posix_memalign");
		return 1;
	}

	printf("%6s %10s %12s %12s %12s %10s %10s\n", "bits", "pass", "stream M/s", "lanes M/s",
	       "vpu M/s", "vs stream", "vs lanes");

	for (k = 0; k < NWIDTHS; k++) {
		unsigned bits = WIDTHS[k], m = lowmask(bits);
		size_t w = words(bits);
		double t0, t1, ts, tl, tv;
		size_t reps, pass;

		for (i = 0; i < BLOCK; i++)
			values[i] = (int)((0x9E3779B9u * (unsigned)(i + 1) ^ ((unsigned)i << 7)) & m);

		/* One packed block, replicated, so every block decodes to the
		 * same values and correctness is checkable everywhere. */
		pack_lanes(lanes_buf, values, bits);
		pack_stream(stream_buf, values, bits);
		for (b = 1; b < blocks; b++) {
			memcpy(lanes_buf + b * w, lanes_buf, w * sizeof *lanes_buf);
			memcpy(stream_buf + b * w, stream_buf, w * sizeof *stream_buf);
		}

		/* Correctness: all three against the original values. */
		memset(out, 0xAA, BLOCK * sizeof *out);
		unpack_stream(out, stream_buf, bits);
		if (memcmp(out, values, BLOCK * sizeof *out)) { printf("b%u: stream FAILED\n", bits); bad++; }
		memset(out, 0xAA, BLOCK * sizeof *out);
		unpack_lanes(out, lanes_buf, bits);
		if (memcmp(out, values, BLOCK * sizeof *out)) { printf("b%u: lanes FAILED\n", bits); bad++; }
		memset(out, 0xAA, BLOCK * sizeof *out);
		KERNEL[k](out, lanes_buf);
		if (memcmp(out, values, BLOCK * sizeof *out)) {
			printf("b%u: vpu FAILED\n", bits);
			bad++;
			for (i = 0; i < BLOCK; i++)
				if (out[i] != values[i]) {
					printf("   first at value %d: got %08x want %08x\n",
					       i, (unsigned)out[i], (unsigned)values[i]);
					break;
				}
		}

		/* Two passes: hot repeats block 0 in place, stream walks all of
		 * them once per repetition. */
		for (pass = 0; pass < 2; pass++) {
			const char *what = pass ? "streaming" : "hot";
			size_t n = pass ? blocks : 1;

			reps = pass ? 8 : 20000;

			t0 = now_s();
			for (i = 0; i < (int)reps; i++)
				for (b = 0; b < n; b++)
					unpack_stream(out + b * BLOCK, stream_buf + b * w, bits);
			t1 = now_s();
			ts = t1 - t0;

			t0 = now_s();
			for (i = 0; i < (int)reps; i++)
				for (b = 0; b < n; b++)
					unpack_lanes(out + b * BLOCK, lanes_buf + b * w, bits);
			t1 = now_s();
			tl = t1 - t0;

			t0 = now_s();
			for (i = 0; i < (int)reps; i++)
				for (b = 0; b < n; b++)
					KERNEL[k](out + b * BLOCK, lanes_buf + b * w);
			t1 = now_s();
			tv = t1 - t0;

			{
				double total = (double)reps * (double)n * BLOCK / 1e6;

				printf("%6u %10s %12.1f %12.1f %12.1f %9.2fx %9.2fx\n", bits, what,
				       total / ts, total / tl, total / tv, ts / tv, tl / tv);
			}
		}
	}

	printf("\ncorrectness: %s\n", bad ? "FAILED" : "OK");
	free(values); free(ref); free(out); free(lanes_buf); free(stream_buf);
	return bad ? 1 : 0;
}
