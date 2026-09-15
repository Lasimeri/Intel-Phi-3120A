/*
 * mandel.c: Mandelbrot set renderer, a CPU benchmark for the card.
 * Escape-time iteration in double precision (x87 on Knights Corner), rows
 * handed out dynamically to N threads, smooth colouring, PNG output through
 * zlib. Compiled on the card by its own clang:
 *   cc -O2 -o mandel mandel.c -lz -lpthread
 *   ./mandel out.png WIDTH HEIGHT MAXITER THREADS [CX CY SCALE]
 * Prints the render time and the throughput in megapixels and
 * giga-iterations per second. See mandel.md.
 */
#include <math.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <zlib.h>

static int W, H, MAXITER;
static double CX = -0.75, CY = 0.0, SCALE = 3.5; /* width of the view in the complex plane */
static uint8_t *image;                           /* W*H*3 RGB */
static volatile int next_row;
static pthread_mutex_t row_lock = PTHREAD_MUTEX_INITIALIZER;
static uint64_t iterations[512];

static void colour(double t, uint8_t *p)
{
	/* A smooth palette: t in [0,1) maps through three sine waves. */
	p[0] = (uint8_t)(255 * (0.5 + 0.5 * sin(6.2832 * (t + 0.00))));
	p[1] = (uint8_t)(255 * (0.5 + 0.5 * sin(6.2832 * (t + 0.33))));
	p[2] = (uint8_t)(255 * (0.5 + 0.5 * sin(6.2832 * (t + 0.67))));
}

static void *worker(void *arg)
{
	long id = (long)arg;
	uint64_t iters = 0;
	double dx = SCALE / W, x0 = CX - SCALE / 2, y0 = CY - dx * H / 2;

	for (;;) {
		pthread_mutex_lock(&row_lock);
		int y = next_row++;
		pthread_mutex_unlock(&row_lock);
		if (y >= H)
			break;
		double ci = y0 + y * dx;
		uint8_t *row = image + (size_t)y * W * 3;
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

static uint32_t crc_table[256];
static void crc_init(void)
{
	for (uint32_t n = 0; n < 256; n++) {
		uint32_t c = n;
		for (int k = 0; k < 8; k++)
			c = c & 1 ? 0xedb88320u ^ (c >> 1) : c >> 1;
		crc_table[n] = c;
	}
}
static uint32_t crc(const uint8_t *buf, size_t len, uint32_t c)
{
	c ^= 0xffffffffu;
	for (size_t i = 0; i < len; i++)
		c = crc_table[(c ^ buf[i]) & 0xff] ^ (c >> 8);
	return c ^ 0xffffffffu;
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
	/* The CRC covers the type and the data together. */
	uint8_t *tmp = malloc(len + 4);
	memcpy(tmp, type, 4);
	if (len)
		memcpy(tmp + 4, data, len);
	uint32_t c = crc(tmp, len + 4, 0);
	free(tmp);
	put32(f, c);
}

static int write_png(const char *path)
{
	FILE *f = fopen(path, "wb");
	if (!f)
		return -1;
	crc_init();
	static const uint8_t sig[8] = { 137, 80, 78, 71, 13, 10, 26, 10 };
	fwrite(sig, 1, 8, f);
	uint8_t ihdr[13] = { W >> 24, W >> 16, W >> 8, W, H >> 24, H >> 16, H >> 8, H, 8, 2, 0, 0, 0 };
	chunk(f, "IHDR", ihdr, 13);
	/* Filter byte 0 in front of every row, then deflate. */
	size_t raw_len = (size_t)H * (W * 3 + 1);
	uint8_t *raw = malloc(raw_len);
	for (int y = 0; y < H; y++) {
		raw[(size_t)y * (W * 3 + 1)] = 0;
		memcpy(raw + (size_t)y * (W * 3 + 1) + 1, image + (size_t)y * W * 3, (size_t)W * 3);
	}
	uLongf zlen = compressBound(raw_len);
	uint8_t *z = malloc(zlen);
	if (compress2(z, &zlen, raw, raw_len, 6) != Z_OK)
		return -1;
	chunk(f, "IDAT", z, zlen);
	chunk(f, "IEND", NULL, 0);
	fclose(f);
	free(raw);
	free(z);
	return 0;
}

static double now(void)
{
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec + ts.tv_nsec * 1e-9;
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
	if (threads < 1 || threads > 512 || W < 1 || H < 1)
		return 2;
	image = malloc((size_t)W * H * 3);
	pthread_t th[512];
	double t0 = now();
	for (long i = 0; i < threads; i++)
		pthread_create(&th[i], NULL, worker, (void *)i);
	uint64_t total = 0;
	for (int i = 0; i < threads; i++) {
		pthread_join(th[i], NULL);
		total += iterations[i];
	}
	double t1 = now();
	double wt = now();
	if (write_png(argv[1]))
		return 1;
	wt = now() - wt;
	printf("%dx%d, maxiter %d, %d threads: %.3f s render (%.2f Mpix/s, %.3f Giter/s), %.3f s png\n",
	       W, H, MAXITER, threads, t1 - t0, W * (double)H / (t1 - t0) / 1e6, total / (t1 - t0) / 1e9, wt);
	return 0;
}
