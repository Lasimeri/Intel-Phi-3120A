/*
 * memhog.c: allocate and touch a given number of MiB, so that the card's
 * memory and its host-RAM swap can be exercised on purpose.
 *
 * The card has 6 GB of GDDR5 and, when the host serves it, a swap device
 * backed by host RAM over the ring (/dev/phiblk1, kernel patch 0026).
 * /tmp is a tmpfs capped at half of RAM, so filling it cannot reach swap;
 * anonymous memory can. This writes one byte per 4 KiB page, which is what
 * forces a page to be resident, then optionally reads it all back and
 * verifies, which is what forces the swapped-out pages to come back over
 * PCIe.
 *
 *   cc -O2 -o memhog memhog.c
 *   ./memhog MIB [--verify] [--hold SECONDS]
 *
 * Exit status 1 on an allocation failure or a verification mismatch.
 * Pages are filled with a value derived from the page index, so a
 * mismatch names the page that came back wrong.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define CHUNK_MIB 256
#define PAGE 4096

static double now_s(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

static unsigned char page_value(size_t page_index)
{
	/* Cheap, order-dependent, and never zero, so a hole reads as a
	 * mismatch rather than as a valid value. */
	return (unsigned char)((page_index * 31u + 7u) | 1u);
}

int main(int argc, char **argv)
{
	size_t mib, chunks, i, j;
	int verify = 0, hold = 0, rc = 0;
	unsigned char **blocks;
	double t0, t1;

	if (argc < 2) {
		fprintf(stderr, "usage: %s MIB [--verify] [--hold SECONDS]\n", argv[0]);
		return 2;
	}
	mib = strtoul(argv[1], NULL, 10);
	for (i = 2; i < (size_t)argc; i++) {
		if (strcmp(argv[i], "--verify") == 0)
			verify = 1;
		else if (strcmp(argv[i], "--hold") == 0 && i + 1 < (size_t)argc)
			hold = atoi(argv[++i]);
		else {
			fprintf(stderr, "%s: unknown argument %s\n", argv[0], argv[i]);
			return 2;
		}
	}
	if (mib == 0 || mib % CHUNK_MIB != 0) {
		fprintf(stderr, "%s: MIB must be a nonzero multiple of %d\n", argv[0], CHUNK_MIB);
		return 2;
	}

	chunks = mib / CHUNK_MIB;
	blocks = calloc(chunks, sizeof(*blocks));
	if (!blocks) {
		perror("calloc");
		return 1;
	}

	printf("allocating %zu MiB in %zu chunks of %d MiB\n", mib, chunks, CHUNK_MIB);
	t0 = now_s();
	for (i = 0; i < chunks; i++) {
		size_t bytes = (size_t)CHUNK_MIB << 20;
		blocks[i] = malloc(bytes);
		if (!blocks[i]) {
			fprintf(stderr, "malloc failed at chunk %zu (%zu MiB in)\n",
				i, i * CHUNK_MIB);
			rc = 1;
			chunks = i;
			break;
		}
		/* One store per page: the page becomes resident here, and the
		 * kernel evicts older pages to swap when GDDR runs out. */
		for (j = 0; j < bytes; j += PAGE) {
			size_t page_index = i * (bytes / PAGE) + j / PAGE;
			blocks[i][j] = page_value(page_index);
		}
		if ((i + 1) % 4 == 0 || i + 1 == chunks)
			printf("  touched %zu MiB (%.1f s)\n",
			       (i + 1) * CHUNK_MIB, now_s() - t0);
	}
	t1 = now_s();
	if (chunks)
		printf("touch: %zu MiB in %.2f s (%.0f MB/s)\n",
		       chunks * CHUNK_MIB, t1 - t0,
		       (double)(chunks * CHUNK_MIB) * 1.048576 / (t1 - t0));

	if (verify && chunks) {
		size_t bad = 0;
		t0 = now_s();
		for (i = 0; i < chunks; i++) {
			size_t bytes = (size_t)CHUNK_MIB << 20;
			for (j = 0; j < bytes; j += PAGE) {
				size_t page_index = i * (bytes / PAGE) + j / PAGE;
				if (blocks[i][j] != page_value(page_index)) {
					if (bad < 8)
						fprintf(stderr,
							"mismatch at chunk %zu page %zu: %02x not %02x\n",
							i, j / PAGE, blocks[i][j],
							page_value(page_index));
					bad++;
				}
			}
		}
		t1 = now_s();
		printf("verify: %zu MiB in %.2f s (%.0f MB/s), %zu mismatches\n",
		       chunks * CHUNK_MIB, t1 - t0,
		       (double)(chunks * CHUNK_MIB) * 1.048576 / (t1 - t0), bad);
		if (bad)
			rc = 1;
	}

	if (hold > 0) {
		printf("holding %d s\n", hold);
		sleep((unsigned)hold);
	}

	for (i = 0; i < chunks; i++)
		free(blocks[i]);
	free(blocks);
	return rc;
}
