/* blkbench: the cost of O_DIRECT reads and writes of /dev/phiblk1 by
 * request size, from the card. Reads a fixed total at each block size and
 * reports the wall time, the per-call mean and the per-call minimum, so a
 * fixed cost per request separates from the link rate. Writes land at
 * 4 GiB into the window, above anything the VPU worker uses (data starts
 * at 1 MiB and grows with the request). Built on the card with cc; see
 * blkbench.md.
 *
 *   blkbench [DEVICE] [TOTAL_MiB] [BS] [huge|4k] [GAP_US]
 *   default /dev/phiblk1, 16 MiB, every size, 4 KiB pages, no gap between calls
 */
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <sys/mman.h>
#include <unistd.h>

static uint64_t now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

static long gap_us;
static const size_t sizes[] = { 4096, 65536, 262144, 524288, 4 << 20, 16 << 20 };
#define WRITE_BASE (4ULL << 30)

static int run(int fd, void *buf, size_t bs, size_t total, int write)
{
    size_t calls = total / bs;
    uint64_t t0 = now_ns(), min = ~0ULL;
    for (size_t i = 0; i < calls; i++) {
        if (gap_us) usleep((useconds_t)gap_us);
        uint64_t c0 = now_ns();
        off_t off = (off_t)(i * bs) + (write ? (off_t)WRITE_BASE : 0);
        ssize_t n = write ? pwrite(fd, buf, bs, off) : pread(fd, buf, bs, off);
        uint64_t c = now_ns() - c0;
        if (n != (ssize_t)bs) { perror(write ? "pwrite" : "pread"); return 1; }
        if (c < min) min = c;
    }
    uint64_t wall = now_ns() - t0;
    printf("%-5s bs=%8zu calls=%5zu wall=%9.3f ms  mean=%9.1f us  min=%9.1f us  %7.0f MB/s\n",
           write ? "write" : "read", bs, calls, wall / 1e6, wall / 1e3 / calls, min / 1e3,
           (double)total / (wall / 1e9) / 1e6);
    return 0;
}

int main(int argc, char **argv)
{
    const char *dev = argc > 1 ? argv[1] : "/dev/phiblk1";
    size_t total = (argc > 2 ? (size_t)atol(argv[2]) : 16) << 20;
    size_t only = argc > 3 ? (size_t)atol(argv[3]) : 0; /* one block size, in bytes */
    int huge = argc > 4 && strcmp(argv[4], "huge") == 0; /* MAP_HUGETLB buffer: one physical segment per 2 MiB */
    gap_us = argc > 5 ? atol(argv[5]) : 0;                /* idle this long between calls: the wake-up cost of an idle path */
    int fd = open(dev, O_RDWR | O_DIRECT);
    if (fd < 0) { perror(dev); return 1; }
    void *buf = NULL;
    size_t cap = sizes[sizeof sizes / sizeof *sizes - 1];
    if (huge) {
        buf = mmap(NULL, cap, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB, -1, 0);
        if (buf == MAP_FAILED) { perror("mmap MAP_HUGETLB (is /proc/sys/vm/nr_hugepages set?)"); return 1; }
    } else if (posix_memalign(&buf, 4096, cap) != 0) return 1;
    memset(buf, 0, sizes[sizeof sizes / sizeof *sizes - 1]);
    for (int w = 0; w < 2; w++)
        for (size_t i = 0; i < sizeof sizes / sizeof *sizes; i++)
            if ((!only || sizes[i] == only) && run(fd, buf, sizes[i], total, w)) return 1;
    return 0;
}
