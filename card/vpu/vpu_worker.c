/* phi-vpu-worker: the card side of the AVX-512 co-processor.
 *
 * Stays resident, polls a doorbell in host memory, and when work arrives
 * runs it across the card's vector units. The kernels it runs are AVX-512
 * machine code translated to this card's own instruction set ahead of
 * time by host/crates/avx512-xlate, so what executes here is the
 * program's own arithmetic, not a reimplementation of it.
 *
 * Data moves through the block device rather than the mapping, because
 * the mapping is uncached and streams at 50 MB/s while the DMA engine
 * behind the block device does 1.2 GB/s. See vpu_proto.md.
 *
 *   phi-vpu-worker [-v] [-s MS] [-i US] [threads]
 *     threads 1 to 228 (57); spin MS after a job before parking (200);
 *     once parked, poll the doorbell every US microseconds (500)
 *
 * The threads are created once. Creating one costs about 0.58 ms on
 * this card, and the first version of this worker created and joined
 * them per request, so that 57 threads took 34.9 ms to do 1.85 ms of
 * work. Every thread in the pool is pinned to its own hardware thread
 * at start-up and stays there.
 */
#define _GNU_SOURCE
#include <fcntl.h>
#include <limits.h>
#include <pthread.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>
#include "vpu_proto.h"

/* Translated from AVX-512 by avx512-xlate; see card/examples/avx512_poly.S */
void poly_kernel_x8(float *d, const float *x, const float *coef, long n);

#define CTRL_BYTES 4096
#define MAX_POOL 227          /* 228 hardware threads less the dispatcher */
#define SPIN_ROUNDS 2000      /* polls of the generation word between clock reads */
static uint64_t spin_ns = 200000000ULL;  /* spin this long after the last job, then park; -s MS */
static long idle_us = 500;               /* doorbell poll interval once idle; -i US */

/* musl on the card ships no linux/futex.h; these are the x86-64 numbers. */
#ifndef SYS_futex
#define SYS_futex 202
#endif
#define FUTEX_WAIT 0
#define FUTEX_WAKE 1

/* Knights Corner has no MFENCE (ISA reference 327364-001, appendix B).
 * A locked read-modify-write is the full fence on this core, the same
 * idiom the vector store kernels in libknc use. It is needed exactly
 * where a store must be visible before a following load: x86 orders
 * everything else. */
#define FULL_FENCE() __asm__ __volatile__("lock addq $0, (%%rsp)" ::: "memory", "cc")
#define COMPILER_BARRIER() __asm__ __volatile__("" ::: "memory")

static int verbose;

static uint64_t now_ns(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)t.tv_sec * 1000000000ULL + (uint64_t)t.tv_nsec;
}

/* Knights Corner numbering: CPU 0 is core 56 thread 3, and CPUs 1+4k
 * through 4+4k are core k. Filling core-major puts one thread on every
 * core before any core gets a second, which is what the two-cycle
 * decoder wants (SSDG 2.1.2). */
static int knc_cpu(int core, int slot)
{
    int cpu = 1 + core * 4 + slot;
    return (cpu >= 228) ? 0 : cpu;
}

static void pin(int cpu)
{
    cpu_set_t set;
    CPU_ZERO(&set);
    CPU_SET(cpu, &set);
    sched_setaffinity(0, sizeof set, &set);
}

static long futex(volatile uint32_t *addr, int op, uint32_t val)
{
    return syscall(SYS_futex, addr, op, val, NULL, NULL, 0);
}

/* ------------------------------------------------------------------ */
/* The pool                                                            */

struct job {
    float *in, *out, *coef;
    long begin, count;
};

static struct {
    int nthreads;               /* pool threads; the dispatcher is one more */
    pthread_t th[MAX_POOL];
    struct job jobs[MAX_POOL + 1];   /* jobs[nthreads] is the dispatcher's own */
    volatile uint32_t gen;      /* bumped once per request */
    volatile uint32_t done;     /* pool threads finished with this generation */
    volatile uint32_t parked;   /* pool threads asleep in the kernel */
} pool;

static void run_job(const struct job *j)
{
    if (j->count > 0) {
        poly_kernel_x8(j->out + j->begin, j->in + j->begin, j->coef, j->count);
    }
}

/* Wait for the generation to move past `seen`.
 *
 * Spin first: a request that follows another closely is picked up in
 * well under a microsecond, with no kernel involved. After SPIN_NS with
 * nothing to do, park in a futex so idle cores are actually idle and the
 * card is not drawing 57 cores' worth of power to wait. The dispatcher
 * wakes parked threads only when there are some, so the common case
 * costs it no system call at all. */
static void wait_for_work(uint32_t seen)
{
    uint64_t t0 = now_ns();
    for (;;) {
        for (int i = 0; i < SPIN_ROUNDS; i++) {
            if (pool.gen != seen) return;
        }
        if (now_ns() - t0 < spin_ns) continue;

        __sync_fetch_and_add(&pool.parked, 1);   /* locked: a full fence */
        /* The kernel compares gen with `seen` atomically against any
         * wake, so a bump that lands between the check above and this
         * call returns EAGAIN rather than sleeping through it. */
        futex(&pool.gen, FUTEX_WAIT, seen);
        __sync_fetch_and_sub(&pool.parked, 1);
        if (pool.gen != seen) return;
        t0 = now_ns();
    }
}

static void *pool_thread(void *arg)
{
    int t = (int)(intptr_t)arg;
    pin(knc_cpu(t % 57, t / 57));
    uint32_t seen = 0;
    for (;;) {
        wait_for_work(seen);
        seen = pool.gen;
        COMPILER_BARRIER();          /* loads of the job follow the load of gen */
        run_job(&pool.jobs[t]);
        __sync_fetch_and_add(&pool.done, 1);   /* locked: results are visible first */
    }
    return NULL;
}

static int pool_start(int nthreads)
{
    pool.nthreads = nthreads;
    for (int t = 0; t < nthreads; t++) {
        if (pthread_create(&pool.th[t], NULL, pool_thread, (void *)(intptr_t)t) != 0) {
            perror("pthread_create");
            return -1;
        }
    }
    return 0;
}

/* Cut n elements into `threads` slices of whole chunks and run them: one
 * on each pool thread, the last one on the calling thread, which would
 * otherwise sit spinning on a core that has work to do. Returns how many
 * slices had any elements. */
static int dispatch(int threads, float *in, float *out, float *coef, long n)
{
    if (threads > pool.nthreads + 1) threads = pool.nthreads + 1;
    if (threads < 1) threads = 1;

    long chunks = (n + VPU_CHUNK - 1) / VPU_CHUNK;
    long per = (chunks + threads - 1) / threads, at = 0;
    int live = 0;
    /* Slice s of `threads` goes to pool thread s, except the last slice,
     * which the dispatcher runs itself (slot nthreads). Pool threads at
     * or past the last slice get nothing this round. The loop visits the
     * dispatcher's slot last, so the slices are handed out in order. */
    for (int t = 0; t <= pool.nthreads; t++) {
        struct job *j = &pool.jobs[t];
        int slice = (t == pool.nthreads) ? threads - 1 : (t < threads - 1 ? t : -1);
        long take = 0;
        if (slice >= 0) {
            take = per;
            if (at + take > chunks) take = chunks - at;
            if (take < 0) take = 0;
        }
        j->in = in; j->out = out; j->coef = coef;
        j->begin = at * VPU_CHUNK;
        j->count = take * VPU_CHUNK;
        if (take > 0 && j->begin + j->count > n) j->count = n - j->begin;
        if (take > 0) { at += take; live++; }
    }

    pool.done = 0;
    COMPILER_BARRIER();          /* the jobs are stored before the generation */
    pool.gen++;
    FULL_FENCE();                /* ...and the generation before parked is read */
    if (pool.parked) futex(&pool.gen, FUTEX_WAKE, INT_MAX);

    run_job(&pool.jobs[pool.nthreads]);

    while (pool.done != (uint32_t)pool.nthreads) { /* spin: alone on this core */ }
    COMPILER_BARRIER();
    return live;
}

/* ------------------------------------------------------------------ */
/* Moving bulk data between the host window and card memory             */

static int blk = -1;

/* O_DIRECT is used for correctness before speed: the host changes this
 * memory behind the card's back, so anything the card's page cache
 * remembers about it is stale, and the worker would compute on whatever
 * the last reader left behind. It also wants block-aligned offsets and
 * lengths, which is why both are rounded up here and why every buffer is
 * page aligned and oversized to match.
 *
 * Large requests matter: the DMA engine reaches 1.2 GB/s with 16 MiB
 * reads and only 185 MB/s with 1 MiB ones. */
static size_t round_up(size_t n) { return (n + VPU_BLOCK - 1) & ~(size_t)(VPU_BLOCK - 1); }

static int pull(void *dst, size_t len, uint64_t off)
{
    size_t want = round_up(len), done = 0;
    while (done < want) {
        ssize_t n = pread(blk, (char *)dst + done, want - done, (off_t)(off + done));
        if (n <= 0) return -1;
        done += (size_t)n;
    }
    return 0;
}

static int push(const void *src, size_t len, uint64_t off)
{
    size_t want = round_up(len), done = 0;
    while (done < want) {
        ssize_t n = pwrite(blk, (const char *)src + done, want - done, (off_t)(off + done));
        if (n <= 0) return -1;
        done += (size_t)n;
    }
    return 0;
}

/* Buffers persist across requests and only grow. Allocating per request
 * meant every request paid a page fault per 4 KiB of data on first
 * touch; the memset here takes those faults once, outside any timing.
 *
 * They come from 2 MiB huge pages when the card has some reserved
 * (/proc/sys/vm/nr_hugepages; scripts/phi-vpu.sh start sets it), else
 * from 4 KiB pages. The difference is the whole transport: /dev/phiblk1
 * posts one record to the host per physically contiguous run of the
 * buffer, and a 4 KiB-paged buffer fresh from malloc is scattered, 15
 * records per 64 KiB and 88 per 512 KiB request, at about 20 us of host
 * work each; a huge-paged buffer is one record per 512 KiB request.
 * Measured 2026-09-22 with blkbench.c on card 0: a 512 KiB pread went
 * from 1.8 ms to 0.34 ms, 16 MiB from 7.4 ms to 5.5 ms (the link). */
#define HUGE_BYTES (2UL << 20)
struct buf { float *p; size_t cap; int huge; };

static int reserve(struct buf *b, size_t len)
{
    size_t need = round_up(len) + VPU_BLOCK;
    if (b->cap >= need) return 0;
    if (b->huge) munmap(b->p, b->cap); else free(b->p);
    b->p = NULL;
    b->cap = 0;
    size_t hneed = (need + HUGE_BYTES - 1) & ~(HUGE_BYTES - 1);
    void *p = mmap(NULL, hneed, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB, -1, 0);
    if (p != MAP_FAILED) {
        b->huge = 1;
        need = hneed;
    } else {
        b->huge = 0;
        p = NULL;
        if (posix_memalign(&p, VPU_BLOCK, need) != 0) return -1;
        if (verbose) fprintf(stderr, "no huge pages for a %zu MiB buffer; 4 KiB pages (slower transport)\n", need >> 20);
    }
    memset(p, 0, need);
    b->p = p;
    b->cap = need;
    return 0;
}

/* ------------------------------------------------------------------ */

int main(int argc, char **argv)
{
    int max_threads = 57;
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "-v") == 0) verbose = 1;
        else if (strcmp(argv[i], "-s") == 0 && i + 1 < argc) spin_ns = (uint64_t)atol(argv[++i]) * 1000000ULL;
        else if (strcmp(argv[i], "-i") == 0 && i + 1 < argc) idle_us = atol(argv[++i]);
        else max_threads = atoi(argv[i]);
    }
    if (max_threads < 1) max_threads = 1;
    if (max_threads > MAX_POOL + 1) max_threads = MAX_POOL + 1;

    int fd = open("/dev/phihost", O_RDWR);
    if (fd < 0) { perror("/dev/phihost"); return 1; }
    volatile unsigned char *ctrl = mmap(NULL, CTRL_BYTES, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (ctrl == MAP_FAILED) { perror("mmap"); return 1; }

    blk = open("/dev/phiblk1", O_RDWR | O_DIRECT);
    if (blk < 0) { perror("/dev/phiblk1 (O_DIRECT)"); return 1; }

    volatile struct vpu_request *req = (volatile struct vpu_request *)(ctrl + VPU_OFF_REQ);
    volatile struct vpu_reply *rep = (volatile struct vpu_reply *)(ctrl + VPU_OFF_REPLY);
    volatile uint64_t *ready = (volatile uint64_t *)(ctrl + VPU_OFF_READY);

    /* The dispatcher lives on CPU 0, which is core 56's last hardware
     * thread, so the pool's core-major fill (core 0 slot 0, core 1 slot
     * 0, ...) never lands a worker on the same core until all 227 other
     * hardware threads are taken. */
    pin(0);
    if (pool_start(max_threads - 1) != 0) return 1;

    printf("phi-vpu-worker: %d threads pinned (%d in the pool plus this one), polling\n",
           max_threads, max_threads - 1);
    fflush(stdout);

    /* The worker owns the reset, not the host.
     *
     * Taking `last` from whatever the window already held meant a request
     * left there by a previous run was indistinguishable from no request
     * at all: the worker would sit polling a sequence number that already
     * matched, and every subsequent request that happened to reuse that
     * number was ignored for ever. Clearing both counters here makes the
     * sequence start from a known point every time the worker starts. */
    req->seq = 0;
    rep->seq = 0;
    rep->status = 0;
    uint64_t last = 0;
    *ready = VPU_MAGIC;

    struct buf in = {0}, out = {0}, coef = {0};

    /* The doorbell is polled flat out for the spin window after the last
     * request, one PCIe read per iteration, which is where the 2.36 us
     * doorbell comes from. After that the poll sleeps between reads so a
     * quiet card is quiet: without this the dispatcher sat at 100 percent
     * of its hardware thread for ever, reading host memory over PCIe a
     * million times a second to learn nothing. nanosleep on this kernel
     * costs about 60 us on top of what is asked (measured 2026-09-22:
     * 10 us asks for 72, 100 us for 162), so the first doorbell after a
     * quiet spell is seen within about idle_us + 60 us. */
    uint64_t idle_since = now_ns();
    unsigned polls = 0;
    for (;;) {
        uint64_t seq = req->seq;
        if (seq == last) {
            /* Keep asserting readiness while idle: the host clears the
             * window before its first request and would otherwise wipe
             * the flag it is about to wait for. */
            *ready = VPU_MAGIC;
            if ((++polls & 63) == 0 && now_ns() - idle_since > spin_ns) {
                struct timespec rest = { 0, (long)idle_us * 1000L };
                nanosleep(&rest, NULL);
            }
            continue;
        }
        last = seq;

        uint64_t t0 = now_ns();
        long n = (long)req->n;
        int threads = (int)req->threads;
        uint32_t kernel = req->kernel;
        uint64_t in_off = req->in_off, out_off = req->out_off;
        uint64_t aux_off = req->aux_off, aux_len = req->aux_len;

        int status = VPU_OK, live = 0;
        uint64_t pull_ns = 0, push_ns = 0, compute_ns = 0;
        size_t bytes = (size_t)n * 4;

        if (n <= 0 || (in_off | out_off | aux_off) % VPU_BLOCK != 0) status = VPU_E_REQUEST;
        else if (kernel != VPU_K_POLY30) status = VPU_E_KERNEL;
        else if (reserve(&in, bytes) || reserve(&out, bytes) || reserve(&coef, aux_len)) status = VPU_E_ALLOC;

        if (status == VPU_OK) {
            uint64_t p0 = now_ns();
            if (pull(in.p, bytes, in_off) || pull(coef.p, aux_len, aux_off)) status = VPU_E_PULL;
            pull_ns = now_ns() - p0;
        }
        if (status == VPU_OK) {
            uint64_t c0 = now_ns();
            live = dispatch(threads, in.p, out.p, coef.p, n);
            compute_ns = now_ns() - c0;
            uint64_t p0 = now_ns();
            if (push(out.p, bytes, out_off)) status = VPU_E_PUSH;
            push_ns = now_ns() - p0;
        }

        rep->compute_ns = compute_ns;
        rep->pull_ns = pull_ns;
        rep->push_ns = push_ns;
        rep->total_ns = now_ns() - t0;
        rep->status = status;
        rep->threads = live;
        COMPILER_BARRIER();
        rep->seq = seq;   /* written last: it is what the host polls */
        idle_since = now_ns();

        if (verbose) {
            printf("seq=%llu kernel=%u n=%ld threads=%d status=%d pull=%.3fms compute=%.3fms push=%.3fms\n",
                   (unsigned long long)seq, kernel, n, live, status,
                   pull_ns / 1e6, compute_ns / 1e6, push_ns / 1e6);
            fflush(stdout);
        }
    }
}
