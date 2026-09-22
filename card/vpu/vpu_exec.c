/* vpu_exec.c: run a region of the host program's code on this card.
 *
 * The host (libphi512, in the program's own SIGILL handler) sends a
 * phase of a region: the program's machine code with its AVX-512
 * instructions rewritten to MVEX in place (a few pages of the 2 MiB
 * chunk they sit in), a thunk area, the register file, and either the
 * exact memory ranges the phase touches (VPU_MODE_RANGES) or nothing
 * about memory (VPU_MODE_DEMAND). vpu_exec.h has the contract.
 *
 *   - Chunks of the program are mapped at the program's own virtual
 *     addresses (MAP_FIXED_NOREPLACE), so branches and RIP-relative
 *     operands need no fixing. A chunk is one of a pool of huge pages
 *     faulted in at start and moved into place with mremap: a fresh huge
 *     page costs its zero-fill (about 1 ms on this core) every time.
 *   - RANGES: the pages of every range are fetched through the mailbox
 *     (a page is fetched once per region), a loop is split across the
 *     pool when the host says so (each thread its own slice of the
 *     induction register, the bound register set to the slice end, so
 *     the last thread's register file is the sequential one), and the
 *     ranges flagged written go back page by page with line masks.
 *   - DEMAND: a SIGSEGV on an unmapped address fetches that chunk whole
 *     and maps it read-only; the first write snapshots it (the pool does
 *     the copy) and makes it writable; at the exit the chunk is compared
 *     with its snapshot in 64-byte lines (the pool again) and only the
 *     pages that changed go back.
 *   - Any instruction fetch outside the region and the thunk area, and
 *     any ud2, is an exit: the exit rip, the integer registers and flags
 *     come from the signal frame, and the interrupted context is pointed
 *     at vpu_exec_exit_stub on the thread's own stack, which stores the
 *     vector registers after sigreturn (kernel patch 0030 keeps them
 *     across the handler) and returns to C.
 *   - What is mapped stays mapped across the phases of one region; the
 *     phase flagged FINAL unmaps it.
 *
 * The signal handlers run on their own stacks (every pool thread has
 * one) and execute no vector instruction. One region runs at a time;
 * inside it, up to 57 threads run a split loop. See vpu_exec.md. */
#define _GNU_SOURCE

#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <ucontext.h>
#include <unistd.h>
#include "vpu_proto.h"
#include "vpu_exec.h"
#include "vpu_exec_regs.h"

#ifndef MAP_FIXED_NOREPLACE
#define MAP_FIXED_NOREPLACE 0x100000
#endif
#define MAX_CHUNKS 1024            /* 2 GiB of the program at once */
#define HP_POOL 256                /* pre-faulted huge pages: 512 MiB of the program at once */
#define SHADOWS 8
#define PAGES_PER_CHUNK (VPU_EXEC_CHUNK / 4096u)
#define COMPILER_BARRIER() asm volatile("" ::: "memory")

/* A mapped chunk of the program. */
struct chunk {
    uint64_t base;
    int hp;                        /* index of the pooled huge page, or -1 for a fresh mapping */
    uint8_t dirty, exec, writable;
    uint8_t demand;                /* filled by a demand fault: stale after the phase, unmapped at its end */
    uint8_t used;                  /* touched by this region: kept mapped for the next */
    void *shadow;                  /* DEMAND: the chunk before its first write */
    uint64_t filled[PAGES_PER_CHUNK / 64];   /* RANGES: pages fetched so far */
};

/* One thread's run of the phase. */
struct ctx {
    struct vpu_regs regs;          /* at 0: the stubs know */
    uint64_t saved_rsp;            /* at 2256: the stubs know */
    uint64_t exit_rip, fault;
    uint32_t kind, ran;            /* kind at 2280: the loop-exit trampoline writes CTX_KIND_TRAMP */
    uint64_t flags_raw;            /* at 2288: lahf in bits 8..15, seto in bit 0, from the trampoline */
} __attribute__((aligned(64)));
#define CTX_KIND_TRAMP 100

static volatile unsigned char *g_ctrl;
static volatile struct vpu_mail *g_mail;
static int g_blk = -1;
static int g_verbose;
static struct vpu_exec g_desc __attribute__((aligned(64)));
static struct chunk g_chunks[MAX_CHUNKS];
static int g_nchunks;
static void *g_hp_home[HP_POOL];   /* the pool's huge pages, at their home addresses when free */
static int g_hp_free[HP_POOL], g_hp_nfree, g_hp_n;
static void *g_shadows[SHADOWS];
static int g_nshadows, g_shadow_used;
static void *g_stages[2];          /* the write-back slots staged here, one per slot */
#define g_stage (g_stages[g_wb_slot & 1])
static void *g_bundle;             /* the descriptor, thunk and code pages, read in one go */
static struct ctx g_ctx[VPU_EXEC_MAX_THREADS];
__thread struct ctx *vpu_exec_tctx;   /* this thread's run, while in the region; the trampoline reads it through fs */
__thread uint64_t vpu_exec_scratch, vpu_exec_scratch2;
#define t_ctx vpu_exec_tctx
static volatile int g_in_exec, g_lock;
static uint64_t g_fetch_ns, g_faults;
static uint32_t g_fetch_slot, g_wb_slot;
static uint64_t g_session;         /* the host process the mapped chunks belong to */

extern int vpu_exec_enter(struct vpu_regs *regs);
extern void vpu_exec_exit_stub(void);

static uint64_t now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

static void lock(void)   { while (__sync_lock_test_and_set(&g_lock, 1)) ; }
static void unlock(void) { __sync_lock_release(&g_lock); }

/* Ask the host for something and wait for its answer. Under the lock. */
static int mail(uint32_t kind, uint64_t addr, uint64_t len, uint32_t slot)
{
    static uint64_t seq;
    g_mail->addr = addr;
    g_mail->len = len;
    g_mail->kind = kind;
    g_mail->slot = slot;
    COMPILER_BARRIER();
    g_mail->seq = ++seq;
    while (g_mail->ack != seq)
        ;   /* an uncached read of host memory per iteration; the host answers within microseconds */
    return g_mail->status;
}

/* ---- the huge page pool ------------------------------------------- */

static void hp_init(void)
{
    for (int i = 0; i < HP_POOL; i++) {
        void *p = mmap(NULL, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB, -1, 0);
        if (p == MAP_FAILED) break;
        ((volatile char *)p)[0] = 0;   /* fault the whole huge page in now */
        g_hp_home[i] = p;
        g_hp_free[g_hp_nfree++] = i;
        g_hp_n++;
    }
}

/* Put a pooled page (or, when the pool is empty, a fresh mapping) at `base`. */
static int hp_map(uint64_t base, int *hp_out)
{
    void *probe = mmap((void *)base, VPU_EXEC_CHUNK, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    if (probe == MAP_FAILED || (uint64_t)probe != base) {
        if (probe != MAP_FAILED) munmap(probe, VPU_EXEC_CHUNK);
        return VPU_EXIT_COLLISION;
    }
    if (g_hp_nfree > 0) {
        int i = g_hp_free[g_hp_nfree - 1];
        munmap(probe, VPU_EXEC_CHUNK);
        void *p = mremap(g_hp_home[i], VPU_EXEC_CHUNK, VPU_EXEC_CHUNK, MREMAP_MAYMOVE | MREMAP_FIXED, (void *)base);
        if (p == MAP_FAILED || (uint64_t)p != base) return VPU_EXIT_COLLISION;
        /* Hold the home address while the page is out, or something mapped
         * there meanwhile would be replaced when the page comes back. */
        mmap(g_hp_home[i], VPU_EXEC_CHUNK, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
        g_hp_nfree--;
        *hp_out = i;
        return 0;
    }
    /* No pooled page left: a fresh mapping, huge if the card still has one. */
    munmap(probe, VPU_EXEC_CHUNK);
    void *p = mmap((void *)base, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE | MAP_HUGETLB, -1, 0);
    if (p == MAP_FAILED)
        p = mmap((void *)base, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    if (p == MAP_FAILED || (uint64_t)p != base) return VPU_EXIT_COLLISION;
    *hp_out = -1;
    return 0;
}

static void hp_unmap(struct chunk *c)
{
    mprotect((void *)c->base, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE);
    if (c->hp >= 0) {
        mremap((void *)c->base, VPU_EXEC_CHUNK, VPU_EXEC_CHUNK, MREMAP_MAYMOVE | MREMAP_FIXED, g_hp_home[c->hp]);
        g_hp_free[g_hp_nfree++] = c->hp;
    } else {
        munmap((void *)c->base, VPU_EXEC_CHUNK);
    }
}

/* ---- chunks --------------------------------------------------------- */

static struct chunk *find_chunk(uint64_t base)
{
    for (int i = 0; i < g_nchunks; i++)
        if (g_chunks[i].base == base) return &g_chunks[i];
    return NULL;
}

/* Map a chunk at `base` (empty). */
static void evict_unused(void)
{
    int keep = 0;
    for (int i = 0; i < g_nchunks; i++) {
        struct chunk *c = &g_chunks[i];
        if (!c->used) { hp_unmap(c); continue; }
        if (keep != i) g_chunks[keep] = *c;
        keep++;
    }
    g_nchunks = keep;
}

static int new_chunk(uint64_t base, int exec, struct chunk **out)
{
    /* Chunks of earlier regions stay mapped to save the next region the
     * mapping; when the pool is dry they go. */
    if (g_hp_nfree == 0 || g_nchunks >= MAX_CHUNKS) evict_unused();
    if (g_nchunks >= MAX_CHUNKS) return VPU_EXIT_LIMIT;
    struct chunk *c = &g_chunks[g_nchunks];
    memset(c, 0, sizeof *c);
    c->base = base;
    c->exec = exec;
    int r = hp_map(base, &c->hp);
    if (r) return r;
    c->writable = 1;
    c->used = 1;
    if (exec) mprotect((void *)base, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE | PROT_EXEC);
    g_nchunks++;
    *out = c;
    return 0;
}

/* Every change of protection flushes the TLB on every CPU a thread of this
 * process runs on, 57 of them: only when it changes. */
static void set_prot(struct chunk *c, int writable)
{
    if (c->writable == writable) return;
    mprotect((void *)c->base, VPU_EXEC_CHUNK, PROT_READ | (writable ? PROT_WRITE : 0) | (c->exec ? PROT_EXEC : 0));
    c->writable = writable;
}

/* Fetch [addr, addr+len) of the program (page aligned) into its chunk,
 * which is mapped and writable; pages fetched before are skipped. */
static int fetch_pages(struct chunk *c, uint64_t addr, uint64_t len)
{
    uint64_t t0 = now_ns();
    uint64_t end = addr + len;
    while (addr < end) {
        uint64_t p = (addr - c->base) / 4096;
        if (c->filled[p / 64] >> (p % 64) & 1) { addr += 4096; continue; }
        /* the run of unfilled pages from here */
        uint64_t run = addr;
        while (run < end) {
            uint64_t q = (run - c->base) / 4096;
            if (c->filled[q / 64] >> (q % 64) & 1) break;
            run += 4096;
        }
        uint64_t n = run - addr;
        if (n > VPU_EXEC_CHUNK) n = VPU_EXEC_CHUNK;
        /* Alternate slots: the host fills the other one with the next piece
         * of the range while this one is read. */
        uint32_t slot = g_fetch_slot++ & 1;
        if (mail(VPU_MAIL_FETCH, addr, n, slot) != 0) return VPU_EXIT_FAULT;
        if (pread(g_blk, (void *)addr, n, VPU_OFF_EXEC_FETCH + (off_t)slot * VPU_EXEC_CHUNK) != (ssize_t)n) return VPU_EXIT_FAULT;
        for (uint64_t a = addr; a < addr + n; a += 4096) {
            uint64_t q = (a - c->base) / 4096;
            c->filled[q / 64] |= 1ULL << (q % 64);
        }
        addr += n;
    }
    g_fetch_ns += now_ns() - t0;
    return 0;
}

/* ---- the pool's help: copies and diffs across threads ---------------- */

struct copy_job { const void *src; void *dst; size_t len; };
static void copy_slice(void *arg, int slice, int n)
{
    struct copy_job *j = arg;
    size_t per = (j->len / n + 63) & ~(size_t)63;
    size_t at = per * (size_t)slice;
    if (at >= j->len) return;
    size_t take = j->len - at < per ? j->len - at : per;
    memcpy((char *)j->dst + at, (const char *)j->src + at, take);
}

struct diff_job { const uint64_t *now, *was; uint64_t *masks; };
static void diff_slice(void *arg, int slice, int n)
{
    struct diff_job *j = arg;
    unsigned per = (PAGES_PER_CHUNK + n - 1) / n;
    unsigned from = per * (unsigned)slice, to = from + per;
    if (to > PAGES_PER_CHUNK) to = PAGES_PER_CHUNK;
    for (unsigned p = from; p < to; p++) {
        uint64_t mask = 0;
        const uint64_t *a = j->now + p * 512, *b = j->was + p * 512;
        for (int line = 0; line < 64; line++) {
            const uint64_t *x = a + line * 8, *y = b + line * 8;
            if ((x[0] ^ y[0]) | (x[1] ^ y[1]) | (x[2] ^ y[2]) | (x[3] ^ y[3]) |
                (x[4] ^ y[4]) | (x[5] ^ y[5]) | (x[6] ^ y[6]) | (x[7] ^ y[7]))
                mask |= 1ULL << line;
        }
        j->masks[p] = mask;
    }
}

static void *shadow_get(void)
{
    if (g_shadow_used < g_nshadows) return g_shadows[g_shadow_used++];
    if (g_nshadows >= SHADOWS) return NULL;
    void *p = mmap(NULL, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB, -1, 0);
    if (p == MAP_FAILED) p = mmap(NULL, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED) return NULL;
    g_shadows[g_nshadows++] = p;
    g_shadow_used = g_nshadows;
    return p;
}

/* ---- write-back ------------------------------------------------------- */

/* Ranges mode writes pages straight from the mapped chunks into the slot
 * (a range's pages are contiguous there, one DMA per chunk); demand mode
 * copies changed pages into the staging buffer first. Either way the
 * table is staged. */
static int g_wb_direct;

static int stage_flush(int n)
{
    if (n == 0) return 0;
    uint32_t slot = g_wb_slot & 1;
    size_t bytes = g_wb_direct ? VPU_WB_TABLE : VPU_WB_TABLE + (size_t)n * 4096;
    if (pwrite(g_blk, g_stage, bytes, VPU_OFF_EXEC_WB + (off_t)slot * VPU_EXEC_CHUNK) != (ssize_t)bytes) return -1;
    /* Acked as soon as the host has it; the ack of the next one means this
     * one was applied, so the other slot is free to fill meanwhile. */
    mail(VPU_MAIL_WRITEBACK, 0, (uint64_t)n, slot);
    g_wb_slot++;
    return 0;
}

/* Ranges mode: `npages` contiguous pages from `addr`, each with its mask. */
static int stage_direct(int *n, uint64_t addr, int npages, uint64_t first_mask, uint64_t last_mask)
{
    while (npages > 0) {
        if (*n == (int)VPU_WB_MAX_PAGES) {
            if (stage_flush(*n) != 0) return -1;
            *n = 0;
        }
        /* After a flush the slot, and with it the staging table, is the other one. */
        struct vpu_wb_page *table = g_stage;
        int take = npages;
        if (*n + take > (int)VPU_WB_MAX_PAGES) take = (int)VPU_WB_MAX_PAGES - *n;
        if (pwrite(g_blk, (const void *)addr, (size_t)take * 4096, VPU_OFF_EXEC_WB + (off_t)(g_wb_slot & 1) * VPU_EXEC_CHUNK + VPU_WB_TABLE + (off_t)*n * 4096) != (ssize_t)take * 4096) return -1;
        for (int i = 0; i < take; i++) {
            table[*n + i].addr = addr + (uint64_t)i * 4096;
            table[*n + i].lines = ~0ULL;
        }
        table[*n].lines &= first_mask;
        table[*n + take - 1].lines &= (npages == take) ? last_mask : ~0ULL;
        first_mask = ~0ULL;
        *n += take;
        addr += (uint64_t)take * 4096;
        npages -= take;
    }
    return 0;
}

/* Stage one page with a line mask; flush when the slot is full. */
static int stage_page(int *n, uint64_t addr, uint64_t mask)
{
    if (*n == (int)VPU_WB_MAX_PAGES) {
        if (stage_flush(*n) != 0) return -1;
        *n = 0;
    }
    /* After a flush the slot, and with it the staging buffer, is the other one. */
    struct vpu_wb_page *table = g_stage;
    unsigned char *pages = (unsigned char *)g_stage + VPU_WB_TABLE;
    table[*n].addr = addr;
    table[*n].lines = mask;
    memcpy(pages + (size_t)*n * 4096, (const void *)addr, 4096);
    (*n)++;
    return 0;
}

/* DEMAND: a dirty chunk against its snapshot. */
static int writeback_diff(struct chunk *c, int *n, uint32_t *pages_out)
{
    static uint64_t masks[PAGES_PER_CHUNK];
    struct diff_job j = { (const uint64_t *)c->base, c->shadow, masks };
    vpu_pool_map(diff_slice, &j, vpu_pool_threads() + 1);
    for (unsigned p = 0; p < PAGES_PER_CHUNK; p++) {
        if (!masks[p]) continue;
        if (stage_page(n, c->base + p * 4096, masks[p]) != 0) return -1;
        (*pages_out)++;
    }
    return 0;
}

/* RANGES: a written range, the lines it covers, chunk by chunk straight
 * from the mapping. */
static int writeback_range(uint64_t addr, uint64_t len, int *n, uint32_t *pages_out)
{
    uint64_t end = addr + len;
    uint64_t first_page = addr & ~4095ULL, last_page = (end - 1) & ~4095ULL;
    unsigned f = (unsigned)((addr - first_page) / 64), l = (unsigned)((end - 1 - last_page) / 64);
    uint64_t first_mask = ~((1ULL << f) - 1);
    uint64_t last_mask = l == 63 ? ~0ULL : ((1ULL << (l + 1)) - 1);
    if (first_page == last_page) first_mask &= last_mask, last_mask = first_mask;
    for (uint64_t page = first_page; page <= last_page;) {
        uint64_t stop = ((page & ~(uint64_t)(VPU_EXEC_CHUNK - 1)) + VPU_EXEC_CHUNK);
        if (stop > last_page + 4096) stop = last_page + 4096;
        int npages = (int)((stop - page) / 4096);
        uint64_t fm = page == first_page ? first_mask : ~0ULL;
        uint64_t lm = stop == last_page + 4096 ? last_mask : ~0ULL;
        if (stage_direct(n, page, npages, fm, lm) != 0) return -1;
        *pages_out += (uint32_t)npages;
        page = stop;
    }
    return 0;
}

/* ---- signals ---------------------------------------------------------- */

/* Capture the state and arrange for the interrupted context to land in
 * the exit stub, on this thread's own stack, with the register file's
 * address in rdi. gregs are in the ucontext order; gpr[] in x86 order. */
static void leave(ucontext_t *uc, uint32_t kind, uint64_t fault)
{
    greg_t *g = uc->uc_mcontext.gregs;
    static const int order[16] = { REG_RAX, REG_RCX, REG_RDX, REG_RBX, REG_RSP, REG_RBP, REG_RSI, REG_RDI,
                                   REG_R8, REG_R9, REG_R10, REG_R11, REG_R12, REG_R13, REG_R14, REG_R15 };
    struct ctx *c = t_ctx;
    c->kind = kind;
    c->exit_rip = (uint64_t)g[REG_RIP];
    c->fault = fault;
    for (int i = 0; i < 16; i++) c->regs.gpr[i] = (uint64_t)g[order[i]];
    c->regs.rflags = (uint64_t)g[REG_EFL];
    c->regs.rip = c->exit_rip;
    g[REG_RIP] = (greg_t)vpu_exec_exit_stub;
    g[REG_RSP] = (greg_t)c->saved_rsp;
    g[REG_RDI] = (greg_t)&c->regs;
}

static int inside(uint64_t rip)
{
    return (rip >= g_desc.region_lo && rip < g_desc.region_hi) ||
           (rip >= g_desc.thunk_addr && rip < g_desc.thunk_addr + g_desc.thunk_len);
}

static void on_segv(int sig, siginfo_t *si, void *ctx)
{
    ucontext_t *uc = ctx;
    uint64_t rip = (uint64_t)uc->uc_mcontext.gregs[REG_RIP];
    uint64_t addr = (uint64_t)si->si_addr;
    if (!g_in_exec || !t_ctx) {
        signal(sig, SIG_DFL);   /* the worker's own bug; die the normal way */
        return;
    }
    __sync_fetch_and_add(&g_faults, 1);
    if (!inside(rip)) { leave(uc, VPU_EXIT_LEFT, addr); return; }
    if (addr == rip || g_desc.mode == VPU_MODE_RANGES) {
        /* An access outside what the host declared: the analysis was wrong,
         * or the program would have crashed here. Either way, the host hears. */
        leave(uc, VPU_EXIT_FAULT, addr);
        return;
    }
    uint64_t base = addr & ~(uint64_t)(VPU_EXEC_CHUNK - 1);
    lock();
    struct chunk *c = find_chunk(base);
    if (c) {
        if (!c->dirty) {
            /* First write: keep what the chunk held, so the exit can tell
             * what the region changed and send only that. */
            void *shadow = shadow_get();
            if (!shadow) { unlock(); leave(uc, VPU_EXIT_LIMIT, addr); return; }
            struct copy_job j = { (const void *)base, shadow, VPU_EXEC_CHUNK };
            vpu_pool_map(copy_slice, &j, vpu_pool_threads() + 1);
            c->shadow = shadow;
            set_prot(c, 1);
            c->dirty = 1;
            unlock();
            return;
        }
        unlock();
        leave(uc, VPU_EXIT_FAULT, addr);
        return;
    }
    int r = new_chunk(base, 0, &c);
    if (r == 0) {
        uint32_t slot = g_fetch_slot++ & 1;
        if (mail(VPU_MAIL_FETCH, base, VPU_EXEC_CHUNK, slot) != 0 ||
            pread(g_blk, (void *)base, VPU_EXEC_CHUNK, VPU_OFF_EXEC_FETCH + (off_t)slot * VPU_EXEC_CHUNK) != (ssize_t)VPU_EXEC_CHUNK)
            r = VPU_EXIT_FAULT;
        else {
            memset(c->filled, 0xff, sizeof c->filled);
            c->demand = 1;
            set_prot(c, 0);
        }
    }
    unlock();
    if (r) leave(uc, (uint32_t)r, addr);
}

static void on_ill(int sig, siginfo_t *si, void *ctx)
{
    ucontext_t *uc = ctx;
    uint64_t rip = (uint64_t)uc->uc_mcontext.gregs[REG_RIP];
    (void)si;
    if (!g_in_exec || !t_ctx) {
        signal(sig, SIG_DFL);
        return;
    }
    __sync_fetch_and_add(&g_faults, 1);
    const unsigned char *b = (const unsigned char *)rip;
    int ud2 = b[0] == 0x0f && b[1] == 0x0b;
    leave(uc, (inside(rip) && !ud2) ? VPU_EXIT_ILLEGAL : VPU_EXIT_LEFT, rip);
}

int vpu_exec_init(void)
{
    static char altstack[256 << 10] __attribute__((aligned(16)));
    stack_t ss = { .ss_sp = altstack, .ss_size = sizeof altstack, .ss_flags = 0 };
    if (sigaltstack(&ss, NULL) != 0) { perror("sigaltstack"); return -1; }
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_flags = SA_SIGINFO | SA_ONSTACK;
    sigemptyset(&sa.sa_mask);
    sa.sa_sigaction = on_segv;
    if (sigaction(SIGSEGV, &sa, NULL) != 0 || sigaction(SIGBUS, &sa, NULL) != 0) { perror("sigaction"); return -1; }
    sa.sa_sigaction = on_ill;
    if (sigaction(SIGILL, &sa, NULL) != 0) { perror("sigaction"); return -1; }
    hp_init();
    for (int i = 0; i < 2; i++) {
        g_stages[i] = mmap(NULL, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB, -1, 0);
        if (g_stages[i] == MAP_FAILED) g_stages[i] = mmap(NULL, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (g_stages[i] == MAP_FAILED) { perror("mmap"); return -1; }
        memset(g_stages[i], 0, VPU_EXEC_CHUNK);
    }
    g_bundle = mmap(NULL, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB, -1, 0);
    if (g_bundle == MAP_FAILED) g_bundle = mmap(NULL, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (g_bundle == MAP_FAILED) { perror("mmap"); return -1; }
    /* Fault both in now: the zero-fill of a huge page is about a millisecond
     * on this core, and it would otherwise land on the first region. */
    memset(g_bundle, 0, VPU_EXEC_CHUNK);
    printf("exec: %d huge pages pooled\n", g_hp_n);
    return 0;
}

/* Drop every mapping: a new program, or the pool ran dry. */
static void unmap_all(void)
{
    for (int i = 0; i < g_nchunks; i++) hp_unmap(&g_chunks[i]);
    g_nchunks = 0;
    g_shadow_used = 0;
}

/* The end of a region: chunks filled by ranges stay mapped for the next
 * region (their page bitmaps cleared, so pages are fetched again: the
 * program changes its memory between regions), chunks filled by demand
 * faults are unmapped (a mapped chunk cannot fault, so it would serve
 * stale data). Unmapping costs a TLB shootdown per chunk; keeping does not. */
static void end_region(void)
{
    int keep = 0;
    for (int i = 0; i < g_nchunks; i++) {
        struct chunk *c = &g_chunks[i];
        if (c->demand) { hp_unmap(c); continue; }
        memset(c->filled, 0, sizeof c->filled);
        c->dirty = 0;
        c->shadow = NULL;
        c->used = 0;
        if (keep != i) g_chunks[keep] = *c;
        keep++;
    }
    g_nchunks = keep;
    g_shadow_used = 0;
}

/* ---- running ------------------------------------------------------------ */

/* Thread `slice` of a split loop, or the only thread. */
static void run_slice(void *arg, int slice, int n)
{
    (void)arg; (void)n;
    struct ctx *c = &g_ctx[slice];
    t_ctx = c;
    c->ran = 1;
    vpu_exec_enter(&c->regs);   /* returns through the exit stub, with c filled by leave() */
    t_ctx = NULL;
}

/* A split loop leaves by a jump, not a signal: 57 threads taking a
 * signal each went one at a time through the kernel's per-process
 * signal lock, about a millisecond. The exit address gets `jmp` to a slot
 * in the thunk area (within rel32 reach of the program's code) holding
 * `jmp [rip]` to vpu_exec_loop_exit in this binary, which finds the
 * thread's context through fs. */
#define TRAMP_SLOT (16u * VPU_EXEC_MAX_THREADS)
extern void vpu_exec_loop_exit(void);
static void write_tramp_slot(void)
{
    unsigned char *s = (unsigned char *)(g_desc.thunk_addr + TRAMP_SLOT);
    uint64_t target = (uint64_t)vpu_exec_loop_exit;
    s[0] = 0xff; s[1] = 0x25; memset(s + 2, 0, 4);   /* jmp qword [rip + 0] */
    memcpy(s + 6, &target, 8);
}
static void patch_loop_exit(void)
{
    unsigned char *at = (unsigned char *)g_desc.loop_exit;
    int32_t rel = (int32_t)((int64_t)(g_desc.thunk_addr + TRAMP_SLOT) - (int64_t)(g_desc.loop_exit + 5));
    at[0] = 0xe9;
    memcpy(at + 1, &rel, 4);
}

/* The entry stub for thread t: rax, then the entry. The loader has no
 * register left for rax after loading the other fifteen. */
static void write_stub(int t, uint64_t rax)
{
    unsigned char *s = (unsigned char *)(g_desc.thunk_addr + 16u * (unsigned)t);
    s[0] = 0x48; s[1] = 0xb8;
    memcpy(s + 2, &rax, 8);
    s[10] = 0xe9;
    int32_t rel = (int32_t)((int64_t)g_desc.entry - (int64_t)((uint64_t)s + 15));
    memcpy(s + 11, &rel, 4);
    s[15] = 0xcc;
}

int vpu_exec_run(volatile unsigned char *ctrl, int blk_fd, int verbose)
{
    g_ctrl = ctrl;
    g_blk = blk_fd;
    g_verbose = verbose;
    g_mail = (volatile struct vpu_mail *)(ctrl + VPU_OFF_MAIL);
    /* The descriptor, the thunk area and the code pages come as one bundle
     * at VPU_OFF_EXEC_CODE, read with one block request: reading the 4.5 KiB
     * descriptor through the uncached mapping was 570 PCIe round trips,
     * 0.4 ms. Only the two sizes that fix the bundle's length are read
     * uncached. */
    uint64_t t_in0 = now_ns();
    uint64_t thunk_len = *(volatile uint64_t *)(ctrl + VPU_OFF_EXEC + 32);
    uint32_t code_pages = *(volatile uint32_t *)(ctrl + VPU_OFF_EXEC + 64);
    if (thunk_len == 0 || thunk_len > VPU_EXEC_THUNK_MAX || thunk_len % 4096 || code_pages > VPU_EXEC_MAX_PAGES) return VPU_E_REQUEST;
    size_t bundle = VPU_EXEC_DESC_PAGES * 4096 + thunk_len + (size_t)code_pages * 4096;
    if (pread(g_blk, g_bundle, bundle, VPU_OFF_EXEC_CODE) != (ssize_t)bundle) return VPU_E_PULL;
    memcpy(&g_desc, g_bundle, sizeof g_desc);
    const unsigned char *b_thunk = (const unsigned char *)g_bundle + VPU_EXEC_DESC_PAGES * 4096;
    const unsigned char *b_code = b_thunk + thunk_len;
    uint64_t t_in = now_ns() - t_in0;
    g_desc.exit_kind = VPU_EXIT_LEFT;
    g_desc.exit_thread = 0;
    g_desc.chunks = g_desc.dirty = g_desc.faults = g_desc.threads_ran = 0;
    g_desc.fetch_ns = g_desc.wb_ns = g_desc.run_ns = 0;
    g_fetch_ns = g_faults = 0;
    int r = 0;

    if (g_desc.code_addr % VPU_EXEC_CHUNK || g_desc.thunk_len == 0 || g_desc.thunk_len > VPU_EXEC_THUNK_MAX ||
        g_desc.thunk_len % 4096 || g_desc.thunk_addr % 4096 || g_desc.code_pages > VPU_EXEC_MAX_PAGES ||
        g_desc.nranges > VPU_EXEC_MAX_RANGES || g_desc.threads == 0 || g_desc.threads > VPU_EXEC_MAX_THREADS)
        return VPU_E_REQUEST;
    if (verbose) {
        printf("exec: region %llx phase mode %u flags %u code %#llx (%u pages) thunk %#llx region %#llx..%#llx entry %#llx threads %u ranges %u\n",
               (unsigned long long)g_desc.region_id, g_desc.mode, g_desc.flags, (unsigned long long)g_desc.code_addr, g_desc.code_pages,
               (unsigned long long)g_desc.thunk_addr, (unsigned long long)g_desc.region_lo, (unsigned long long)g_desc.region_hi,
               (unsigned long long)g_desc.entry, g_desc.threads, g_desc.nranges);
        fflush(stdout);
    }

    uint64_t t_load0 = now_ns();
    /* The code chunk and the thunk area: fresh on the first phase, kept after. */
    if (g_desc.reserved[0] != g_session) {
        /* Another program: its addresses mean other memory. */
        unmap_all();
        g_session = g_desc.reserved[0];
    }
    if (g_desc.flags & VPU_EXEC_FIRST) end_region();
    struct chunk *code = find_chunk(g_desc.code_addr), *thunk = find_chunk(g_desc.thunk_addr & ~(uint64_t)(VPU_EXEC_CHUNK - 1));
    if (!code && (r = new_chunk(g_desc.code_addr, 1, &code)) != 0) goto fail;
    code->used = 1;
    set_prot(code, 1);
    for (uint32_t i = 0; i < g_desc.code_pages; i++) {
        uint64_t a = g_desc.code_page[i];
        if (a < g_desc.code_addr || a >= g_desc.code_addr + VPU_EXEC_CHUNK) { r = VPU_EXIT_FAULT; g_desc.fault_addr = a; goto fail; }
        memcpy((void *)a, b_code + (size_t)i * 4096, 4096);
        uint64_t q = (a - g_desc.code_addr) / 4096;
        code->filled[q / 64] |= 1ULL << (q % 64);
    }
    uint64_t tbase = g_desc.thunk_addr & ~(uint64_t)(VPU_EXEC_CHUNK - 1);
    if (tbase == g_desc.code_addr) thunk = code;
    if (!thunk && (r = new_chunk(tbase, 1, &thunk)) != 0) goto fail;
    thunk->used = 1;
    set_prot(thunk, 1);
    memcpy((void *)g_desc.thunk_addr, b_thunk, g_desc.thunk_len);
    for (uint64_t a = g_desc.thunk_addr; a < g_desc.thunk_addr + g_desc.thunk_len; a += 4096) {
        uint64_t q = (a - tbase) / 4096;
        thunk->filled[q / 64] |= 1ULL << (q % 64);
    }

    uint64_t t_load = now_ns() - t_load0;
    /* RANGES: every declared page, fetched once per region. */
    if (g_desc.mode == VPU_MODE_RANGES) {
        for (uint32_t i = 0; i < g_desc.nranges; i++) {
            uint64_t a = g_desc.ranges[i].addr & ~4095ULL, e = (g_desc.ranges[i].addr + g_desc.ranges[i].len + 4095) & ~4095ULL;
            while (a < e) {
                uint64_t base = a & ~(uint64_t)(VPU_EXEC_CHUNK - 1), stop = base + VPU_EXEC_CHUNK < e ? base + VPU_EXEC_CHUNK : e;
                struct chunk *c = find_chunk(base);
                if (!c && (r = new_chunk(base, 0, &c)) != 0) { g_desc.fault_addr = a; goto fail; }
                set_prot(c, 1);
                c->used = 1;
                if (g_desc.ranges[i].flags & VPU_RANGE_DENSE) {
                    /* Written whole by the phase: only its first and last pages
                     * hold bytes of the program that the write-back could carry. */
                    if ((r = fetch_pages(c, a, 4096)) != 0) { g_desc.fault_addr = a; goto fail; }
                    if (stop - 4096 > a && (r = fetch_pages(c, stop - 4096, 4096)) != 0) { g_desc.fault_addr = a; goto fail; }
                    for (uint64_t p = a; p < stop; p += 4096) {
                        uint64_t q = (p - c->base) / 4096;
                        c->filled[q / 64] |= 1ULL << (q % 64);
                    }
                } else if ((r = fetch_pages(c, a, stop - a)) != 0) { g_desc.fault_addr = a; goto fail; }
                a = stop;
            }
        }
    }
    /* The threads: one register file each, the induction register and
     * the bound register set to the thread's slice. */
    int T = (int)g_desc.threads;
    if (T > vpu_pool_threads() + 1) T = vpu_pool_threads() + 1;
    uint64_t iters = 0;
    if (T > 1) {
        int64_t step = g_desc.step;
        int64_t span = (int64_t)(g_desc.ind_end - g_desc.ind_start);
        if (step == 0 || (step > 0 && span <= 0) || (step < 0 && span >= 0)) { r = VPU_EXIT_SPLIT; goto fail; }
        iters = (uint64_t)((span + step + (step > 0 ? -1 : 1)) / step);
        if ((uint64_t)T > iters) T = (int)iters;
        if (T < 1) T = 1;
    }
    uint64_t per = T > 1 ? (iters + (uint64_t)T - 1) / (uint64_t)T : 0;
    for (int t = 0; t < T; t++) {
        struct ctx *c = &g_ctx[t];
        memset(c, 0, sizeof *c);
        c->regs = g_desc.regs;
        if (T > 1) {
            uint64_t s = g_desc.ind_start + (uint64_t)((int64_t)(per * (uint64_t)t) * g_desc.step);
            uint64_t e = t == T - 1 ? g_desc.ind_end : g_desc.ind_start + (uint64_t)((int64_t)(per * (uint64_t)(t + 1)) * g_desc.step);
            c->regs.gpr[g_desc.ind_reg] = s;
            /* The last thread keeps the original bound: it runs the loop's
             * own tail and leaves the registers and flags the sequential
             * run would have. The others stop at their slice end. */
            if (t != T - 1) c->regs.gpr[g_desc.bound_reg] = e;
        }
        c->regs.rip = g_desc.thunk_addr + 16u * (unsigned)t;
        write_stub(t, c->regs.gpr[0]);
    }
    if (T > 1) {
        write_tramp_slot();
        patch_loop_exit();
    }
    /* The code and the thunk are executable from here. In demand mode they
     * are read-only as well, so a write to data sharing their chunks is
     * caught and snapshotted like any other; in ranges mode nothing is
     * snapshotted and a declared write range may live in them. The stubs
     * were written just above, while the thunk was still writable. */
    set_prot(code, g_desc.mode == VPU_MODE_RANGES || code->dirty);
    if (thunk != code) set_prot(thunk, g_desc.mode == VPU_MODE_RANGES || thunk->dirty);

    uint64_t t0 = now_ns();
    g_in_exec = 1;
    if (T == 1) run_slice(NULL, 0, 1);
    else vpu_pool_map(run_slice, NULL, T);
    g_in_exec = 0;
    g_desc.run_ns = now_ns() - t0;
    g_desc.threads_ran = (uint32_t)T;

    /* The outcome: every thread must have left at the loop exit (a split)
     * or the one thread anywhere; the last thread's register file is the
     * sequential one. */
    struct ctx *last = &g_ctx[T - 1];
    for (int t = 0; t < T; t++) {
        struct ctx *c = &g_ctx[t];
        if (c->kind == CTX_KIND_TRAMP) {
            /* lahf: SF ZF - AF - PF - CF in ah, the low byte of rflags; seto in al */
            uint64_t f = c->regs.rflags & ~0x8d5ULL;
            f |= (c->flags_raw >> 8) & 0xd5;
            f |= (c->flags_raw & 1) << 11;
            c->regs.rflags = f;
            c->exit_rip = g_desc.loop_exit;
            c->regs.rip = g_desc.loop_exit;
            c->kind = VPU_EXIT_LEFT;
        }
        if (c->kind != VPU_EXIT_LEFT || (T > 1 && c->exit_rip != g_desc.loop_exit)) {
            g_desc.exit_kind = c->kind == VPU_EXIT_LEFT ? VPU_EXIT_SPLIT : c->kind;
            g_desc.exit_thread = (uint32_t)t;
            g_desc.exit_rip = c->exit_rip;
            g_desc.fault_addr = c->fault;
            last = c;
            break;
        }
    }
    if (g_desc.exit_kind == VPU_EXIT_LEFT) g_desc.exit_rip = last->exit_rip;
    g_desc.regs = last->regs;

    /* What the phase wrote, back to the host. Only on a clean exit: a
     * phase that faulted may have written half its output, and the host
     * retries it (in demand mode) from the same registers and the same
     * input memory, which a partial write-back could have clobbered. */
    if (g_desc.exit_kind != VPU_EXIT_LEFT) goto done;
    uint64_t w0 = now_ns();
    uint32_t pages = 0;
    int n = 0;
    g_wb_direct = g_desc.mode == VPU_MODE_RANGES;
    if (g_desc.mode == VPU_MODE_RANGES) {
        for (uint32_t i = 0; i < g_desc.nranges; i++)
            if (g_desc.ranges[i].flags & VPU_RANGE_WRITE)
                if (writeback_range(g_desc.ranges[i].addr, g_desc.ranges[i].len, &n, &pages) != 0) { g_desc.exit_kind = VPU_EXIT_FAULT; break; }
    } else {
        for (int i = 0; i < g_nchunks; i++) {
            struct chunk *c = &g_chunks[i];
            if (!c->dirty || !c->shadow) continue;
            if (writeback_diff(c, &n, &pages) != 0) { g_desc.exit_kind = VPU_EXIT_FAULT; break; }
            /* the snapshot is stale now; the next phase snapshots again on its first write */
            c->dirty = 0;
            c->shadow = NULL;
            set_prot(c, 0);
        }
        g_shadow_used = 0;
    }
    if (stage_flush(n) != 0) g_desc.exit_kind = VPU_EXIT_FAULT;
    g_desc.dirty = pages;
    g_desc.wb_ns = now_ns() - w0;
    goto done;

fail:
    g_desc.exit_kind = (uint32_t)r;
done:
    g_desc.chunks = (uint32_t)g_nchunks;
    g_desc.faults = (uint32_t)g_faults;
    g_desc.fetch_ns = g_fetch_ns;
    uint64_t t_un0 = now_ns();
    if ((g_desc.flags & VPU_EXEC_FINAL) || g_desc.exit_kind != VPU_EXIT_LEFT) end_region();
    uint64_t t_un = now_ns() - t_un0, t_out0 = now_ns();
    memcpy((void *)(ctrl + VPU_OFF_EXEC), &g_desc, sizeof g_desc);
    uint64_t t_out = now_ns() - t_out0;
    if (verbose) { printf("exec: stages: desc in %.3f ms, code+thunk load %.3f ms, unmap %.3f ms, desc out %.3f ms\n", t_in / 1e6, t_load / 1e6, t_un / 1e6, t_out / 1e6); fflush(stdout); }
    if (verbose) {
        printf("exec: exit kind %u at %#llx (thread %u of %u), %u chunks, %u pages back, %u faults; fetch %.3f ms, run %.3f ms, wb %.3f ms\n",
               g_desc.exit_kind, (unsigned long long)g_desc.exit_rip, g_desc.exit_thread, g_desc.threads_ran,
               g_desc.chunks, g_desc.dirty, g_desc.faults, g_desc.fetch_ns / 1e6, g_desc.run_ns / 1e6, g_desc.wb_ns / 1e6);
        fflush(stdout);
    }
    return VPU_OK;
}

/* Enter the region: the thread's callee-saved registers are pushed and
 * its stack pointer kept in the context (offset 2256, right after the
 * register file), the register file is loaded (vector unit first, then
 * flags, then the integer registers, rsp last), and control goes to
 * regs.rip, the thread's entry stub. The exit stub is where leave() sends
 * the interrupted context: rdi is the register file, rsp the thread's. */
asm(".text\n"
    ".globl vpu_exec_enter\n"
    ".type vpu_exec_enter, @function\n"
    "vpu_exec_enter:\n"
    "\tpush %rbx\n\tpush %rbp\n\tpush %r12\n\tpush %r13\n\tpush %r14\n\tpush %r15\n"
    "\tmov %rsp, 2256(%rdi)\n"
    "\tmov %rdi, %rax\n"
    KNC_VPU_RESTORE_ASM
    "\tpush 2240(%rax)\n\tpopfq\n"
    "\tmov 2112+8(%rax), %rcx\n\tmov 2112+16(%rax), %rdx\n\tmov 2112+24(%rax), %rbx\n"
    "\tmov 2112+40(%rax), %rbp\n\tmov 2112+48(%rax), %rsi\n\tmov 2112+56(%rax), %rdi\n"
    "\tmov 2112+64(%rax), %r8\n\tmov 2112+72(%rax), %r9\n\tmov 2112+80(%rax), %r10\n\tmov 2112+88(%rax), %r11\n"
    "\tmov 2112+96(%rax), %r12\n\tmov 2112+104(%rax), %r13\n\tmov 2112+112(%rax), %r14\n\tmov 2112+120(%rax), %r15\n"
    "\tmov 2112+32(%rax), %rsp\n"
    "\tjmp *2248(%rax)\n"
    ".size vpu_exec_enter, .-vpu_exec_enter\n"
    ".globl vpu_exec_loop_exit\n"
    ".type vpu_exec_loop_exit, @function\n"
    "vpu_exec_loop_exit:\n"
    "\tmov %rax, %fs:vpu_exec_scratch@tpoff\n"
    "\tlahf\n\tseto %al\n"
    "\tmov %rax, %fs:vpu_exec_scratch2@tpoff\n"
    "\tmov %fs:vpu_exec_tctx@tpoff, %rax\n"
    "\tmov %rcx, 2112+8(%rax)\n\tmov %rdx, 2112+16(%rax)\n\tmov %rbx, 2112+24(%rax)\n\tmov %rsp, 2112+32(%rax)\n"
    "\tmov %rbp, 2112+40(%rax)\n\tmov %rsi, 2112+48(%rax)\n\tmov %rdi, 2112+56(%rax)\n"
    "\tmov %r8, 2112+64(%rax)\n\tmov %r9, 2112+72(%rax)\n\tmov %r10, 2112+80(%rax)\n\tmov %r11, 2112+88(%rax)\n"
    "\tmov %r12, 2112+96(%rax)\n\tmov %r13, 2112+104(%rax)\n\tmov %r14, 2112+112(%rax)\n\tmov %r15, 2112+120(%rax)\n"
    "\tmov %fs:vpu_exec_scratch@tpoff, %rcx\n\tmov %rcx, 2112(%rax)\n"
    "\tmov %fs:vpu_exec_scratch2@tpoff, %rcx\n\tmov %rcx, 2288(%rax)\n"
    "\tmovl $100, 2280(%rax)\n"
    "\tmov 2256(%rax), %rsp\n"
    KNC_VPU_SAVE_ASM
    "\tpop %r15\n\tpop %r14\n\tpop %r13\n\tpop %r12\n\tpop %rbp\n\tpop %rbx\n"
    "\txor %eax, %eax\n"
    "\tret\n"
    ".size vpu_exec_loop_exit, .-vpu_exec_loop_exit\n"
    ".globl vpu_exec_exit_stub\n"
    ".type vpu_exec_exit_stub, @function\n"
    "vpu_exec_exit_stub:\n"
    "\tmov %rdi, %rax\n"
    KNC_VPU_SAVE_ASM
    "\tmov 2256(%rdi), %rsp\n"
    "\tpop %r15\n\tpop %r14\n\tpop %r13\n\tpop %r12\n\tpop %rbp\n\tpop %rbx\n"
    "\txor %eax, %eax\n"
    "\tret\n"
    ".size vpu_exec_exit_stub, .-vpu_exec_exit_stub\n");
