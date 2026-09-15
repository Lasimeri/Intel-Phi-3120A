/*
 * mandel-vpu.c: the Mandelbrot benchmark on the vector units. On the card
 * the inner loop is mandel_vpu.S, hand-encoded Knights Corner vector code
 * working on 8 doubles per instruction; on the host it is AVX2 intrinsics
 * on 4 doubles. Both do the same operations in the same order (one fused
 * multiply-add for the imaginary part, plain multiplies and adds for the
 * rest, round to nearest), so the two machines produce the same iteration
 * counts. Three timed passes over the image:
 *   1. iterate: escape count and |z|^2 at exit for every pixel (the vector part)
 *   2. colour: the smooth palette through libm (sin, log, sqrt), scalar
 *   3. deflate in 128 KiB pieces on all threads, then write (as mandel.c)
 * Build on the card:  cc -O2 -o mandel-vpu mandel-vpu.c mandel_vpu.S -lz -lpthread
 * Build on the host:  cc -O2 -mavx2 -mfma -o mandel-vpu mandel-vpu.c -lz -lpthread -lm
 *   ./mandel-vpu out.png WIDTH HEIGHT MAXITER THREADS [CX CY SCALE]
 * See mandel-vpu.md.
 */
#include <math.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <zlib.h>

#if defined(__AVX2__) && defined(__FMA__)
#include <immintrin.h>
#define LANES 4
#define LANES_NAME "AVX2"
#else
#define LANES 8
#define LANES_NAME "KNC VPU"
/* mandel_vpu.S: c[0..7] = cr, c[8..15] = ci; out[0..7] = count, out[8..15] = |z|^2. 64-byte aligned. */
void mandel8_vpu(const double *c, double *out, int maxiter);
#endif

#define MAX_THREADS 512
#define PIECE (128 * 1024)
#define DICT 32768

static int W, H, MAXITER;
static double CX = -0.75, CY = 0.0, SCALE = 3.5;
static uint16_t *counts; /* W*H escape counts */
static float *mags;      /* W*H |z|^2 at exit */
static uint8_t *raw;     /* PNG scanlines: filter byte then RGB */
static size_t stride, raw_len;
static volatile int next_row, next_piece;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static uint64_t useful[MAX_THREADS], trips[MAX_THREADS];

struct piece {
	uint8_t *out;
	size_t out_len, in_len;
	uint32_t crc, adler;
};
static struct piece *pieces;
static int npieces;

static void die(const char *what)
{
	fprintf(stderr, "mandel-vpu: %s\n", what);
	exit(1);
}

static double now(void)
{
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec + ts.tv_nsec * 1e-9;
}

#if LANES == 4
/* The same loop as mandel_vpu.S, on 4 lanes: masked updates by blend. */
static void mandel4_avx2(const double *c, double *out, int maxiter)
{
	__m256d cr = _mm256_load_pd(c), ci = _mm256_load_pd(c + 4);
	__m256d zr = _mm256_setzero_pd(), zi = zr, zr2 = zr, zi2 = zr, n = zr;
	const __m256d two = _mm256_set1_pd(2.0), four = _mm256_set1_pd(4.0), one = _mm256_set1_pd(1.0);
	__m256d active = _mm256_castsi256_pd(_mm256_set1_epi64x(-1));

	for (int it = 0; it < maxiter; it++) {
		__m256d mag = _mm256_add_pd(zr2, zi2);
		active = _mm256_and_pd(active, _mm256_cmp_pd(mag, four, _CMP_LT_OQ));
		if (!_mm256_movemask_pd(active))
			break;
		__m256d t = _mm256_mul_pd(zr, two);
		__m256d zin = _mm256_fmadd_pd(t, zi, ci);
		__m256d zrn = _mm256_add_pd(_mm256_sub_pd(zr2, zi2), cr);
		zi = _mm256_blendv_pd(zi, zin, active);
		zr = _mm256_blendv_pd(zr, zrn, active);
		zr2 = _mm256_mul_pd(zr, zr); /* frozen lanes recompute their old value exactly */
		zi2 = _mm256_mul_pd(zi, zi);
		n = _mm256_add_pd(n, _mm256_and_pd(active, one));
	}
	_mm256_store_pd(out, n);
	_mm256_store_pd(out + 4, _mm256_add_pd(zr2, zi2));
}
#define KERNEL mandel4_avx2
#else
#define KERNEL mandel8_vpu
#endif

/* Pass 1: rows handed out dynamically, LANES pixels per kernel call. */
static void *iterate_worker(void *arg)
{
	long id = (long)arg;
	double c[2 * LANES] __attribute__((aligned(64)));
	double o[2 * LANES] __attribute__((aligned(64)));
	double dx = SCALE / W, x0 = CX - SCALE / 2, y0 = CY - dx * H / 2;
	uint64_t sum = 0, loops = 0;

	for (;;) {
		pthread_mutex_lock(&lock);
		int y = next_row++;
		pthread_mutex_unlock(&lock);
		if (y >= H)
			break;
		double ci = y0 + y * dx;
		for (int x = 0; x < W; x += LANES) {
			for (int i = 0; i < LANES; i++) {
				int xx = x + i < W ? x + i : W - 1;
				c[i] = x0 + xx * dx;
				c[LANES + i] = ci;
			}
			KERNEL(c, o, MAXITER);
			double most = 0;
			for (int i = 0; i < LANES && x + i < W; i++) {
				counts[(size_t)y * W + x + i] = (uint16_t)o[i];
				mags[(size_t)y * W + x + i] = (float)o[LANES + i];
				sum += (uint64_t)o[i];
				if (o[i] > most)
					most = o[i];
			}
			loops += (uint64_t)most;
		}
	}
	useful[id] = sum;
	trips[id] = loops;
	return NULL;
}

static void colour(double t, uint8_t *p)
{
	p[0] = (uint8_t)(255 * (0.5 + 0.5 * sin(6.2832 * (t + 0.00))));
	p[1] = (uint8_t)(255 * (0.5 + 0.5 * sin(6.2832 * (t + 0.33))));
	p[2] = (uint8_t)(255 * (0.5 + 0.5 * sin(6.2832 * (t + 0.67))));
}

/* Pass 2: the palette, as mandel.c, from the stored counts and magnitudes. */
static void *colour_worker(void *arg)
{
	(void)arg;
	for (;;) {
		pthread_mutex_lock(&lock);
		int y = next_row++;
		pthread_mutex_unlock(&lock);
		if (y >= H)
			break;
		uint8_t *row = raw + (size_t)y * stride;
		row[0] = 0;
		row++;
		for (int x = 0; x < W; x++) {
			int n = counts[(size_t)y * W + x];
			if (n == MAXITER) {
				row[x * 3] = row[x * 3 + 1] = row[x * 3 + 2] = 0;
			} else {
				double m = mags[(size_t)y * W + x];
				double nu = n + 1 - log(log(sqrt(m))) / log(2.0);
				colour(fmod(nu * 0.02, 1.0), row + x * 3);
			}
		}
	}
	return NULL;
}

/* Pass 3: parallel deflate, the pigz method; see mandel.c and mandel.md. */
static void *deflate_worker(void *arg)
{
	(void)arg;
	z_stream s;
	memset(&s, 0, sizeof s);
	if (deflateInit2(&s, 6, Z_DEFLATED, -15, 8, Z_DEFAULT_STRATEGY) != Z_OK)
		die("deflateInit2");
	for (;;) {
		pthread_mutex_lock(&lock);
		int k = next_piece++;
		pthread_mutex_unlock(&lock);
		if (k >= npieces)
			break;
		struct piece *p = &pieces[k];
		size_t off = (size_t)k * PIECE;
		size_t len = raw_len - off < PIECE ? raw_len - off : PIECE;
		int last = k == npieces - 1;
		deflateReset(&s);
		if (k > 0)
			deflateSetDictionary(&s, raw + off - DICT, DICT);
		size_t cap = deflateBound(&s, len) + 64, n = 0;
		p->out = malloc(cap);
		if (!p->out)
			die("malloc");
		s.next_in = raw + off;
		s.avail_in = len;
		for (;;) {
			s.next_out = p->out + n;
			s.avail_out = cap - n;
			int rc = deflate(&s, last ? Z_FINISH : Z_SYNC_FLUSH);
			n = cap - s.avail_out;
			if (rc == Z_STREAM_END)
				break;
			if (rc != Z_OK && rc != Z_BUF_ERROR)
				die("deflate");
			if (s.avail_in == 0 && s.avail_out != 0)
				break;
			cap *= 2;
			p->out = realloc(p->out, cap);
			if (!p->out)
				die("realloc");
		}
		p->out_len = n;
		p->in_len = len;
		p->crc = crc32(0, p->out, n);
		p->adler = adler32(1, raw + off, len);
	}
	deflateEnd(&s);
	return NULL;
}

static void put32(FILE *f, uint32_t v)
{
	uint8_t b[4] = { v >> 24, v >> 16, v >> 8, v };
	fwrite(b, 1, 4, f);
}

static void chunk(FILE *f, const char *type, const uint8_t *data, size_t len)
{
	put32(f, (uint32_t)len);
	fwrite(type, 1, 4, f);
	if (len)
		fwrite(data, 1, len, f);
	uint32_t c = crc32(0, (const uint8_t *)type, 4);
	if (len)
		c = crc32(c, data, len);
	put32(f, c);
}

static void write_idat(FILE *f)
{
	static const uint8_t hdr[2] = { 0x78, 0x9c };
	uint32_t crc = crc32(crc32(0, (const uint8_t *)"IDAT", 4), hdr, 2);
	uint32_t adler = 1;
	size_t total = 2 + 4;
	for (int k = 0; k < npieces; k++) {
		total += pieces[k].out_len;
		crc = crc32_combine(crc, pieces[k].crc, pieces[k].out_len);
		adler = adler32_combine(adler, pieces[k].adler, pieces[k].in_len);
	}
	uint8_t trailer[4] = { adler >> 24, adler >> 16, adler >> 8, adler };
	crc = crc32(crc, trailer, 4);
	put32(f, (uint32_t)total);
	fwrite("IDAT", 1, 4, f);
	fwrite(hdr, 1, 2, f);
	for (int k = 0; k < npieces; k++)
		fwrite(pieces[k].out, 1, pieces[k].out_len, f);
	fwrite(trailer, 1, 4, f);
	put32(f, crc);
}

static void run_threads(int threads, void *(*fn)(void *))
{
	static pthread_t th[MAX_THREADS];
	for (long i = 0; i < threads; i++)
		if (pthread_create(&th[i], NULL, fn, (void *)i))
			die("pthread_create");
	for (int i = 0; i < threads; i++)
		pthread_join(th[i], NULL);
}

int main(int argc, char **argv)
{
	if (argc < 6) {
		fprintf(stderr, "usage: %s out.png WIDTH HEIGHT MAXITER THREADS [CX CY SCALE]\n", argv[0]);
		return 2;
	}
	W = atoi(argv[2]);
	H = atoi(argv[3]);
	MAXITER = atoi(argv[4]);
	int threads = atoi(argv[5]);
	if (argc >= 9) {
		CX = atof(argv[6]);
		CY = atof(argv[7]);
		SCALE = atof(argv[8]);
	}
	if (threads < 1 || threads > MAX_THREADS || W < 1 || H < 1 || MAXITER < 1 || MAXITER > 65535)
		return 2;
	stride = (size_t)W * 3 + 1;
	raw_len = stride * H;
	counts = malloc((size_t)W * H * sizeof *counts);
	mags = malloc((size_t)W * H * sizeof *mags);
	raw = malloc(raw_len);
	if (!counts || !mags || !raw)
		die("malloc");

	double t0 = now();
	run_threads(threads, iterate_worker);
	double t1 = now();
	next_row = 0;
	run_threads(threads, colour_worker);
	double t2 = now();
	npieces = (int)((raw_len + PIECE - 1) / PIECE);
	pieces = calloc(npieces, sizeof *pieces);
	if (!pieces)
		die("calloc");
	run_threads(threads, deflate_worker);
	double t3 = now();

	FILE *f = fopen(argv[1], "wb");
	if (!f)
		die("fopen");
	static const uint8_t sig[8] = { 137, 80, 78, 71, 13, 10, 26, 10 };
	fwrite(sig, 1, 8, f);
	uint8_t ihdr[13] = { W >> 24, W >> 16, W >> 8, W, H >> 24, H >> 16, H >> 8, H, 8, 2, 0, 0, 0 };
	chunk(f, "IHDR", ihdr, 13);
	write_idat(f);
	chunk(f, "IEND", NULL, 0);
	fclose(f);
	double t4 = now();

	uint64_t sum = 0, loops = 0;
	for (int i = 0; i < threads; i++) {
		sum += useful[i];
		loops += trips[i];
	}
	printf("%dx%d, maxiter %d, %d threads, %s (%d lanes): %.3f s iterate (%.2f Giter/s useful, "
	       "%.2f Giter/s lane, %.0f%% lanes busy), %.3f s colour, %.3f s deflate, %.3f s write, %.3f s whole\n",
	       W, H, MAXITER, threads, LANES_NAME, LANES, t1 - t0, sum / (t1 - t0) / 1e9,
	       loops * (double)LANES / (t1 - t0) / 1e9, 100.0 * sum / (loops * (double)LANES), t2 - t1, t3 - t2,
	       t4 - t3, t4 - t0);
	return 0;
}
