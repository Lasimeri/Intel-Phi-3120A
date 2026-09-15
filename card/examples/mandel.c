/*
 * mandel.c: Mandelbrot set renderer, a CPU benchmark for the card.
 * Escape-time iteration in double precision (x87 on Knights Corner), rows
 * handed out dynamically to N threads, smooth colouring, PNG output through
 * zlib with the deflate stage parallelised the way pigz does it. Compiled on
 * the card by its own clang:
 *   cc -O2 -o mandel mandel.c -lz -lpthread
 *   ./mandel out.png WIDTH HEIGHT MAXITER THREADS [CX CY SCALE]
 * Prints the render time, the throughput in megapixels and giga-iterations
 * per second, and the PNG stage time. See mandel.md.
 */
#include <math.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <zlib.h>

#define MAX_THREADS 512
#define PIECE (128 * 1024) /* deflate work unit, as pigz's default block */
#define DICT 32768         /* deflate window carried from the previous piece */

static int W, H, MAXITER;
static double CX = -0.75, CY = 0.0, SCALE = 3.5; /* width of the view in the complex plane */
static uint8_t *raw;                             /* H rows of (1 + W*3): PNG filter byte then RGB */
static size_t stride, raw_len;
static volatile int next_row, next_piece;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static uint64_t iterations[MAX_THREADS];

/* One compressed piece of the scanline buffer. */
struct piece {
	uint8_t *out;
	size_t out_len, in_len;
	uint32_t crc, adler;
};
static struct piece *pieces;
static int npieces;

static void die(const char *what)
{
	fprintf(stderr, "mandel: %s\n", what);
	exit(1);
}

static double now(void)
{
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec + ts.tv_nsec * 1e-9;
}

static void colour(double t, uint8_t *p)
{
	/* A smooth palette: t in [0,1) maps through three sine waves. */
	p[0] = (uint8_t)(255 * (0.5 + 0.5 * sin(6.2832 * (t + 0.00))));
	p[1] = (uint8_t)(255 * (0.5 + 0.5 * sin(6.2832 * (t + 0.33))));
	p[2] = (uint8_t)(255 * (0.5 + 0.5 * sin(6.2832 * (t + 0.67))));
}

/* Render stage: each thread takes the next unrendered row until none is left. */
static void *render_worker(void *arg)
{
	long id = (long)arg;
	uint64_t iters = 0;
	double dx = SCALE / W, x0 = CX - SCALE / 2, y0 = CY - dx * H / 2;

	for (;;) {
		pthread_mutex_lock(&lock);
		int y = next_row++;
		pthread_mutex_unlock(&lock);
		if (y >= H)
			break;
		double ci = y0 + y * dx;
		uint8_t *row = raw + (size_t)y * stride;
		row[0] = 0; /* PNG filter type: none */
		row++;
		for (int x = 0; x < W; x++) {
			double cr = x0 + x * dx, zr = 0, zi = 0, zr2 = 0, zi2 = 0;
			int n = 0;
			while (n < MAXITER && zr2 + zi2 < 4.0) {
				zi = 2 * zr * zi + ci;
				zr = zr2 - zi2 + cr;
				zr2 = zr * zr;
				zi2 = zi * zi;
				n++;
			}
			iters += n;
			if (n == MAXITER) {
				row[x * 3] = row[x * 3 + 1] = row[x * 3 + 2] = 0;
			} else {
				double nu = n + 1 - log(log(sqrt(zr2 + zi2))) / log(2.0);
				colour(fmod(nu * 0.02, 1.0), row + x * 3);
			}
		}
	}
	iterations[id] = iters;
	return NULL;
}

/*
 * Deflate stage. Each piece is compressed by its own raw deflate state
 * (windowBits -15: no zlib header or trailer) primed with the 32 KiB that
 * precede it, so the decoder's window matches and the ratio stays close to
 * a single stream. Every piece but the last ends with Z_SYNC_FLUSH, which
 * emits an empty stored block and leaves the output byte aligned without a
 * final bit; the last ends with Z_FINISH. Concatenated, the pieces form one
 * valid deflate stream (pigz, "Parallel deflate", pigz.c).
 */
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
				break; /* the sync flush is complete */
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

/* A PNG chunk with data in one buffer; the CRC covers type and data. */
static void chunk(FILE *f, const char *type, const uint8_t *data, size_t len)
{
	put32(f, (uint32_t)len);
	fwrite(type, 1, 4, f);
	if (len)
		fwrite(data, 1, len, f);
	/* crc32() with a null buffer returns the initial value, so skip empty data. */
	uint32_t c = crc32(0, (const uint8_t *)type, 4);
	if (len)
		c = crc32(c, data, len);
	put32(f, c);
}

/*
 * The IDAT chunk: zlib header, the concatenated pieces, adler32 trailer.
 * The chunk CRC and the stream adler32 come from the per piece values
 * through crc32_combine and adler32_combine, so nothing here rescans the
 * data; the only serial work is the fwrite.
 */
static void write_idat(FILE *f)
{
	static const uint8_t hdr[2] = { 0x78, 0x9c }; /* deflate, 32 KiB window, level 6 */
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
	if (threads < 1 || threads > MAX_THREADS || W < 1 || H < 1)
		return 2;
	stride = (size_t)W * 3 + 1;
	raw_len = stride * H;
	raw = malloc(raw_len);
	if (!raw)
		die("malloc");

	double t0 = now();
	run_threads(threads, render_worker);
	double t1 = now();
	uint64_t total = 0;
	for (int i = 0; i < threads; i++)
		total += iterations[i];

	npieces = (int)((raw_len + PIECE - 1) / PIECE);
	pieces = calloc(npieces, sizeof *pieces);
	if (!pieces)
		die("calloc");
	run_threads(threads, deflate_worker);
	double t2 = now();

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
	double t3 = now();

	size_t zbytes = 0;
	for (int k = 0; k < npieces; k++)
		zbytes += pieces[k].out_len;
	printf("%dx%d, maxiter %d, %d threads: %.3f s render (%.2f Mpix/s, %.3f Giter/s), "
	       "%.3f s deflate (%d pieces, %.1f MB to %.1f MB), %.3f s write\n",
	       W, H, MAXITER, threads, t1 - t0, W * (double)H / (t1 - t0) / 1e6, total / (t1 - t0) / 1e9, t2 - t1,
	       npieces, raw_len / 1e6, zbytes / 1e6, t3 - t2);
	return 0;
}
