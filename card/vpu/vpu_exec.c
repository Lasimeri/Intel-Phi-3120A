/* vpu_exec.c: run a region of the host program's code on this card.
 *
 * The host (libphi512, in the program's own SIGILL handler) sends the
 * region as a chunk of the program's machine code with its AVX-512
 * instructions rewritten to MVEX in place, a thunk area, and the full
 * register file (vpu_exec.h). This engine:
 *
 *   - maps the code chunk and the thunk area at the program's own
 *     virtual addresses (MAP_FIXED_NOREPLACE), so branches and
 *     RIP-relative operands need no fixing;
 *   - loads the register file and jumps in (vpu_exec_enter);
 *   - maps the rest of the program's memory as the code touches it: a
 *     SIGSEGV on an unmapped address fetches that 256 KiB chunk from the
 *     host through the mailbox and maps it read-only at the same
 *     address; the first write to a chunk faults again, and the chunk is
 *     made writable and marked dirty;
 *   - treats any instruction fetch outside the region and the thunk area
 *     as the region's exit (the host put ud2 there, or the code jumped
 *     out): the exit rip and the register file go back to the host;
 *   - writes every dirty chunk back through the mailbox and unmaps
 *     everything.
 *
 * The signal handlers run on their own stack and execute no vector
 * instruction; kernel patch 0030 keeps the vector unit across them. The
 * exit state is captured by pointing the interrupted context at
 * vpu_exec_exit_stub, which runs after sigreturn with the region's
 * registers live. One region runs at a time, on the dispatcher thread.
 * See vpu_exec.md. */
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
#define MAX_CHUNKS 8192           /* 2 GiB of the program at once */
#define COMPILER_BARRIER() asm volatile("" ::: "memory")

static volatile unsigned char *g_ctrl;
static volatile struct vpu_mail *g_mail;
static int g_blk = -1;
static int g_verbose;
static struct vpu_exec g_desc __attribute__((aligned(64)));
static struct { uint64_t addr; uint8_t dirty; uint8_t exec; void *shadow; } g_chunks[MAX_CHUNKS];
static void *g_shadows[64];       /* snapshots of chunks before their first write, reused */
static int g_nshadows;
static void *g_stage;             /* the write-back slot staged here (one huge page when available) */
static int g_nchunks;
static volatile int g_in_exec;
uint64_t vpu_exec_worker_rsp;     /* the dispatcher's stack, kept across the region */
static uint64_t g_fetch_ns, g_faults;

static void *shadow_get(void);
extern int vpu_exec_enter(struct vpu_regs *regs);
extern void vpu_exec_exit_stub(void);

static uint64_t now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

/* Ask the host for something and wait for its answer. */
static int mail(uint32_t kind, uint64_t addr, uint64_t len)
{
    static uint64_t seq;
    g_mail->addr = addr;
    g_mail->len = len;
    g_mail->kind = kind;
    COMPILER_BARRIER();
    g_mail->seq = ++seq;
    while (g_mail->ack != seq)
        ;   /* an uncached read of host memory per iteration; the host answers within microseconds */
    return g_mail->status;
}

static int find_chunk(uint64_t base)
{
    for (int i = 0; i < g_nchunks; i++)
        if (g_chunks[i].addr == base) return i;
    return -1;
}

/* Map one chunk of the program at its own address and fill it. */
static int map_chunk(uint64_t base, size_t len, uint64_t window_off, int exec)
{
    if (g_nchunks >= MAX_CHUNKS) return VPU_EXIT_LIMIT;
    /* A huge page when the card has one (scripts/phi-vpu.sh start reserves
     * them): the chunk is then one physical run, four block records to
     * move instead of 512. Else 4 KiB pages, which work but cost more. */
    void *p = MAP_FAILED;
    if (len == VPU_EXEC_CHUNK && base % (2u << 20) == 0)
        p = mmap((void *)base, len, PROT_READ | PROT_WRITE,
                 MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE | MAP_HUGETLB, -1, 0);
    if (p == MAP_FAILED)
        p = mmap((void *)base, len, PROT_READ | PROT_WRITE,
                 MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    if (p == MAP_FAILED) return VPU_EXIT_COLLISION;
    if ((uint64_t)p != base) { munmap(p, len); return VPU_EXIT_COLLISION; }
    if (pread(g_blk, p, len, (off_t)window_off) != (ssize_t)len) { munmap(p, len); return VPU_EXIT_FAULT; }
    mprotect(p, len, exec ? PROT_READ | PROT_EXEC : PROT_READ);
    g_chunks[g_nchunks].addr = base;
    g_chunks[g_nchunks].dirty = 0;
    g_chunks[g_nchunks].exec = exec;
    g_chunks[g_nchunks].shadow = NULL;
    g_nchunks++;
    return 0;
}

/* Fetch a chunk of the program's memory from the host. */
static int fetch_chunk(uint64_t base)
{
    uint64_t t0 = now_ns();
    if (g_nchunks >= MAX_CHUNKS) return VPU_EXIT_LIMIT;
    if (mail(VPU_MAIL_FETCH, base, VPU_EXEC_CHUNK) != 0) return VPU_EXIT_FAULT;
    int r = map_chunk(base, VPU_EXEC_CHUNK, VPU_OFF_EXEC_FETCH, 0);
    g_fetch_ns += now_ns() - t0;
    return r;
}

/* Capture the state and arrange for the interrupted context to land in
 * the exit stub, on the dispatcher's stack, with the register file's
 * address in rdi. gregs are in the ucontext order; gpr[] in x86 order. */
static void leave(ucontext_t *uc, uint32_t kind, uint64_t fault)
{
    greg_t *g = uc->uc_mcontext.gregs;
    static const int order[16] = { REG_RAX, REG_RCX, REG_RDX, REG_RBX, REG_RSP, REG_RBP, REG_RSI, REG_RDI,
                                   REG_R8, REG_R9, REG_R10, REG_R11, REG_R12, REG_R13, REG_R14, REG_R15 };
    g_desc.exit_kind = kind;
    g_desc.exit_rip = (uint64_t)g[REG_RIP];
    g_desc.fault_addr = fault;
    for (int i = 0; i < 16; i++) g_desc.regs.gpr[i] = (uint64_t)g[order[i]];
    g_desc.regs.rflags = (uint64_t)g[REG_EFL];
    g_desc.regs.rip = g_desc.exit_rip;
    g[REG_RIP] = (greg_t)vpu_exec_exit_stub;
    g[REG_RSP] = (greg_t)vpu_exec_worker_rsp;
    g[REG_RDI] = (greg_t)&g_desc.regs;
    g_in_exec = 0;
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
    if (!g_in_exec) {
        signal(sig, SIG_DFL);   /* the worker's own bug; die the normal way */
        return;
    }
    g_faults++;
    if (g_verbose) { printf("exec: segv rip %#llx addr %#llx\n", (unsigned long long)rip, (unsigned long long)addr); fflush(stdout); }
    if (!inside(rip)) { leave(uc, VPU_EXIT_LEFT, addr); return; }
    if (addr == rip) { leave(uc, VPU_EXIT_FAULT, addr); return; }
    uint64_t base = addr & ~(uint64_t)(VPU_EXEC_CHUNK - 1);
    int i = find_chunk(base);
    if (i >= 0) {
        if (!g_chunks[i].dirty) {
            /* First write: keep what the chunk held, so the exit can tell
             * what the region changed and send only that. */
            void *shadow = shadow_get();
            if (!shadow) { leave(uc, VPU_EXIT_LIMIT, addr); return; }
            memcpy(shadow, (void *)base, VPU_EXEC_CHUNK);
            g_chunks[i].shadow = shadow;
            mprotect((void *)base, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE | (g_chunks[i].exec ? PROT_EXEC : 0));
            g_chunks[i].dirty = 1;
            return;
        }
        leave(uc, VPU_EXIT_FAULT, addr);
        return;
    }
    int r = fetch_chunk(base);
    if (r) leave(uc, (uint32_t)r, addr);
}

static void on_ill(int sig, siginfo_t *si, void *ctx)
{
    ucontext_t *uc = ctx;
    uint64_t rip = (uint64_t)uc->uc_mcontext.gregs[REG_RIP];
    (void)si;
    if (!g_in_exec) {
        signal(sig, SIG_DFL);
        return;
    }
    g_faults++;
    if (g_verbose) { printf("exec: sigill rip %#llx\n", (unsigned long long)rip); fflush(stdout); }
    leave(uc, inside(rip) ? VPU_EXIT_ILLEGAL : VPU_EXIT_LEFT, rip);
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
    return 0;
}

static void unmap_all(void)
{
    for (int i = 0; i < g_nchunks; i++)
        munmap((void *)g_chunks[i].addr, VPU_EXEC_CHUNK);
    g_nchunks = 0;
}

/* A 2 MiB buffer for a chunk's snapshot, from a small pool that grows to
 * the most chunks any region has written at once. */
static int g_shadow_used;         /* snapshots handed out during this region */
static void *shadow_get(void)
{
    if (g_shadow_used < g_nshadows) return g_shadows[g_shadow_used++];
    if (g_nshadows >= (int)(sizeof g_shadows / sizeof *g_shadows)) return NULL;
    void *p = NULL;
    if (posix_memalign(&p, 4096, VPU_EXEC_CHUNK) != 0) return NULL;
    g_shadows[g_nshadows++] = p;
    g_shadow_used = g_nshadows;
    return p;
}

/* The staging buffer for the write-back slot: one huge page when the
 * card has one (four block records instead of 512), else 4 KiB pages. */
static void *stage_get(void)
{
    if (g_stage) return g_stage;
    void *p = mmap(NULL, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB, -1, 0);
    if (p == MAP_FAILED) p = mmap(NULL, VPU_EXEC_CHUNK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED) return NULL;
    g_stage = p;
    return p;
}

/* Send the staged pages: the table, then the pages, one mail. */
static int stage_flush(struct vpu_wb_page *table, int n)
{
    if (n == 0) return 0;
    size_t bytes = VPU_WB_TABLE + (size_t)n * 4096;
    if (pwrite(g_blk, g_stage, bytes, VPU_OFF_EXEC_WB) != (ssize_t)bytes) return -1;
    mail(VPU_MAIL_WRITEBACK, 0, (uint64_t)n);
    return 0;
}

/* Compare a dirty chunk with its snapshot, 64-byte line by line, and
 * stage every page that changed with the mask of its changed lines. */
static int writeback_chunk(uint64_t base, const void *shadow, uint32_t *pages_out)
{
    struct vpu_wb_page *table = g_stage;
    unsigned char *pages = (unsigned char *)g_stage + VPU_WB_TABLE;
    int n = 0;
    const uint64_t *now = (const uint64_t *)base, *was = shadow;
    for (uint64_t p = 0; p < VPU_EXEC_CHUNK / 4096; p++) {
        uint64_t mask = 0;
        const uint64_t *a = now + p * 512, *b = was + p * 512;
        for (int line = 0; line < 64; line++) {
            const uint64_t *x = a + line * 8, *y = b + line * 8;
            if ((x[0] ^ y[0]) | (x[1] ^ y[1]) | (x[2] ^ y[2]) | (x[3] ^ y[3]) |
                (x[4] ^ y[4]) | (x[5] ^ y[5]) | (x[6] ^ y[6]) | (x[7] ^ y[7]))
                mask |= 1ULL << line;
        }
        if (!mask) continue;
        if (n == (int)VPU_WB_MAX_PAGES) {
            if (stage_flush(table, n) != 0) return -1;
            n = 0;
        }
        table[n].addr = base + p * 4096;
        table[n].lines = mask;
        memcpy(pages + (size_t)n * 4096, a, 4096);
        n++;
        (*pages_out)++;
    }
    return stage_flush(table, n);
}

int vpu_exec_run(volatile unsigned char *ctrl, int blk_fd, int verbose)
{
    g_ctrl = ctrl;
    g_blk = blk_fd;
    g_verbose = verbose;
    g_mail = (volatile struct vpu_mail *)(ctrl + VPU_OFF_MAIL);
    memcpy(&g_desc, (const void *)(ctrl + VPU_OFF_EXEC), sizeof g_desc);
    g_desc.exit_kind = VPU_EXIT_LEFT;
    g_desc.chunks = g_desc.dirty = g_desc.faults = 0;
    g_desc.fetch_ns = g_desc.wb_ns = g_desc.run_ns = 0;
    g_fetch_ns = g_faults = 0;
    g_shadow_used = 0;
    if (!stage_get()) return VPU_E_ALLOC;

    if (g_desc.code_len == 0 || g_desc.code_len > VPU_EXEC_CHUNK || g_desc.code_len % VPU_BLOCK ||
        g_desc.code_addr % VPU_EXEC_CHUNK || g_desc.thunk_len == 0 || g_desc.thunk_len > VPU_EXEC_THUNK_MAX ||
        g_desc.thunk_len % VPU_BLOCK || g_desc.thunk_addr % VPU_BLOCK)
        return VPU_E_REQUEST;

    if (verbose) {
        printf("exec: code %#llx+%llu thunk %#llx+%llu region %#llx..%#llx entry %#llx rip %#llx rsp %#llx\n",
               (unsigned long long)g_desc.code_addr, (unsigned long long)g_desc.code_len,
               (unsigned long long)g_desc.thunk_addr, (unsigned long long)g_desc.thunk_len,
               (unsigned long long)g_desc.region_lo, (unsigned long long)g_desc.region_hi,
               (unsigned long long)g_desc.entry, (unsigned long long)g_desc.regs.rip, (unsigned long long)g_desc.regs.gpr[4]);
        fflush(stdout);
    }
    int r = map_chunk(g_desc.code_addr, VPU_EXEC_CHUNK, VPU_OFF_EXEC_CODE, 1);
    if (r == 0) r = map_chunk(g_desc.thunk_addr, g_desc.thunk_len, VPU_OFF_EXEC_THUNK, 1);
    if (r) {
        g_desc.exit_kind = (uint32_t)r;
        g_desc.fault_addr = g_nchunks ? g_desc.thunk_addr : g_desc.code_addr;
        unmap_all();
        memcpy((void *)(ctrl + VPU_OFF_EXEC), &g_desc, sizeof g_desc);
        return VPU_OK;
    }
    /* The thunk area holds the entry stub, which loads rax and jumps to
     * the faulting instruction; rax is the one register the loader has no
     * hands left for. */
    g_desc.regs.rip = g_desc.entry;
    if (verbose) { printf("exec: mapped code and thunk, entering\n"); fflush(stdout); }

    uint64_t t0 = now_ns();
    g_in_exec = 1;
    vpu_exec_enter(&g_desc.regs);   /* returns through the exit stub, with g_desc filled by leave() */
    g_desc.run_ns = now_ns() - t0;

    /* Only what the region changed goes back: each written chunk against
     * its snapshot, line by line, so nothing the host kept changing
     * meanwhile (its own handler's memory) is overwritten with a stale copy. */
    uint64_t w0 = now_ns();
    uint32_t pages = 0;
    for (int i = 0; i < g_nchunks; i++) {
        if (!g_chunks[i].dirty || !g_chunks[i].shadow) continue;
        if (writeback_chunk(g_chunks[i].addr, g_chunks[i].shadow, &pages) != 0)
            g_desc.exit_kind = VPU_EXIT_FAULT;
    }
    g_desc.dirty = pages;
    g_desc.wb_ns = now_ns() - w0;
    g_desc.chunks = (uint32_t)g_nchunks;
    g_desc.faults = (uint32_t)g_faults;
    g_desc.fetch_ns = g_fetch_ns;
    unmap_all();
    memcpy((void *)(ctrl + VPU_OFF_EXEC), &g_desc, sizeof g_desc);
    if (verbose)
        printf("exec: region %#llx..%#llx exit kind %u at %#llx, %u chunks (%u dirty), %u faults, fetch %.3f ms, run %.3f ms, wb %.3f ms\n",
               (unsigned long long)g_desc.region_lo, (unsigned long long)g_desc.region_hi, g_desc.exit_kind,
               (unsigned long long)g_desc.exit_rip, g_desc.chunks, g_desc.dirty, g_desc.faults,
               g_desc.fetch_ns / 1e6, g_desc.run_ns / 1e6, g_desc.wb_ns / 1e6);
    return VPU_OK;
}

/* Enter the region: callee-saved registers and the stack pointer are kept
 * in globals, the register file is loaded (vector unit first, then flags,
 * then the integer registers, rsp last), and control goes to regs.rip,
 * the entry stub. Offsets into struct vpu_regs: zmm 0, k 2048, gpr 2112,
 * rflags 2240, rip 2248. The exit stub is where leave() sends the
 * interrupted context: rdi is the register file, rsp the dispatcher's. */
asm(".text\n"
    ".globl vpu_exec_enter\n"
    ".type vpu_exec_enter, @function\n"
    "vpu_exec_enter:\n"
    "\tpush %rbx\n\tpush %rbp\n\tpush %r12\n\tpush %r13\n\tpush %r14\n\tpush %r15\n"
    "\tmov %rsp, vpu_exec_worker_rsp(%rip)\n"
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
    ".globl vpu_exec_exit_stub\n"
    ".type vpu_exec_exit_stub, @function\n"
    "vpu_exec_exit_stub:\n"
    "\tmov %rdi, %rax\n"
    KNC_VPU_SAVE_ASM
    "\tmov vpu_exec_worker_rsp(%rip), %rsp\n"
    "\tpop %r15\n\tpop %r14\n\tpop %r13\n\tpop %r12\n\tpop %rbp\n\tpop %rbx\n"
    "\txor %eax, %eax\n"
    "\tret\n"
    ".size vpu_exec_exit_stub, .-vpu_exec_exit_stub\n");
