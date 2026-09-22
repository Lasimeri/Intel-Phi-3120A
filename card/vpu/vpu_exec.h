/* vpu_exec.h: the seamless path's contract between libphi512 on the host
 * and the card worker. The host hands the card a region of the program's
 * own machine code (its AVX-512 instructions rewritten to the card's
 * MVEX encoding in place, thunks for the few that need a sequence, and
 * ud2 at every exit) and the full register file. The card maps the
 * program's memory at the program's own addresses, runs the region on
 * its vector units, and returns the register file, where the region left
 * off, and what the region wrote. See vpu_exec.md.
 *
 * Two modes. VPU_MODE_RANGES: the host has worked out every address the
 * phase touches (`ranges`); the card fetches exactly those pages, runs,
 * and writes the ranges flagged as written back; a loop can be split
 * across the pool (`threads` > 1). VPU_MODE_DEMAND: the card maps 2 MiB
 * chunks of the program as the code touches them, through the mailbox,
 * snapshots a chunk at its first write and returns the lines that
 * changed. A region can run in phases (the same `region_id`): what the
 * card mapped stays mapped until a phase flagged VPU_EXEC_FINAL.
 *
 * Offsets are in the shared window (host: /dev/shm/phi-hostmem[-N];
 * card: /dev/phihost, and /dev/phiblk1 for bulk). Mirrored in
 * host/crates/phi-vpu/src/proto.rs; tools/vpu-layout-check.c checks. */
#ifndef VPU_EXEC_H
#define VPU_EXEC_H
#include <stdint.h>

#define VPU_EXEC_CHUNK      (2u << 20)     /* the unit of the program's memory the card maps: one huge page */
#define VPU_EXEC_THUNK_MAX  (64u << 10)    /* thunk area: the entry stubs (first 1 KiB), then out-of-line sequences */
#define VPU_EXEC_MAX_RANGES 64
#define VPU_EXEC_MAX_PAGES  64             /* code pages sent per phase */
#define VPU_EXEC_MAX_THREADS 64            /* entry stubs in the thunk area */
#define VPU_EXEC_DESC_PAGES 2              /* the descriptor at the head of the bundle at VPU_OFF_EXEC_CODE */

#define VPU_OFF_MAIL        4096           /* struct vpu_mail: the card asks, the host answers */
#define VPU_OFF_EXEC        8192           /* struct vpu_exec: the phase and the register file */
#define VPU_OFF_EXEC_FETCH  (32u << 20)    /* two slots of a chunk each: a range the host copied for the card; the card alternates, the host fills the other ahead */
#define VPU_OFF_EXEC_CODE   (VPU_OFF_EXEC_FETCH + 2 * VPU_EXEC_CHUNK)   /* the bundle: the descriptor (VPU_EXEC_DESC_PAGES pages), the thunk area, the code pages, contiguous */
#define VPU_OFF_EXEC_WB     (VPU_OFF_EXEC_CODE + VPU_EXEC_CHUNK)        /* two slots: pages the card wrote, for the host to apply; the card writes one while the host applies the other */

/* The register file, in the order the card's own save sequence uses
 * (asm/knc_vpu.h in the card kernel): zmm0..31 at 0, k0..7 at 2048. gpr[]
 * is in x86 encoding order: rax rcx rdx rbx rsp rbp rsi rdi r8..r15. */
struct vpu_regs {
    uint8_t  zmm[32][64];
    uint16_t k[8];
    uint8_t  pad[48];
    uint64_t gpr[16];
    uint64_t rflags;
    uint64_t rip;
};

/* A range of the program's memory the phase touches. */
#define VPU_RANGE_WRITE     1   /* written by the phase: write it back at the exit */
#define VPU_RANGE_DENSE     2   /* every byte of it is written and none read: only its edge pages need fetching */
struct vpu_range {
    uint64_t addr;
    uint64_t len;
    uint32_t flags;
    uint32_t pad;
};

#define VPU_MODE_DEMAND     0
#define VPU_MODE_RANGES     1
#define VPU_EXEC_FINAL      1   /* unmap everything at the end of this phase */
#define VPU_EXEC_FIRST      2   /* the first phase of this region: map the code chunk and the thunk area afresh */

/* Why the phase stopped. */
#define VPU_EXIT_LEFT       0   /* control left the phase (its ud2, or a jump elsewhere): rip is where the host resumes */
#define VPU_EXIT_FAULT      1   /* a data access the host could not serve, or one outside the ranges: fault_addr, rip */
#define VPU_EXIT_ILLEGAL    2   /* an instruction inside the region the card refused: rip */
#define VPU_EXIT_COLLISION  3   /* a program address is already in use on the card: fault_addr */
#define VPU_EXIT_LIMIT      4   /* more chunks, pages or threads than the card handles */
#define VPU_EXIT_SPLIT      5   /* a thread of a split loop left at the wrong place: exit_thread, rip */

struct vpu_exec {
    uint64_t region_id;
    uint32_t mode;          /* VPU_MODE_* */
    uint32_t flags;         /* VPU_EXEC_* */
    uint64_t code_addr;     /* program address of the code chunk (chunk aligned) */
    uint64_t thunk_addr;    /* program address the thunk area is mapped at (page aligned; free on the host and the card) */
    uint64_t thunk_len;     /* bytes, a multiple of 4096, at most VPU_EXEC_THUNK_MAX; its bytes follow the descriptor in the bundle */
    uint64_t region_lo;     /* rip in [lo, hi) or in the thunk area is inside; anywhere else is an exit */
    uint64_t region_hi;
    uint64_t entry;         /* the first instruction to run */
    uint32_t code_pages;    /* pages in VPU_OFF_EXEC_CODE, page i at slot + i * 4096, its address in code_page[i] */
    uint32_t nranges;
    /* a split loop: every thread runs [entry, loop_exit) with its own slice of the induction register */
    uint32_t threads;       /* 1: no split */
    uint32_t ind_reg;       /* x86 index of the induction register */
    uint32_t bound_reg;     /* the register that holds the loop bound: each thread's end goes there */
    uint32_t pad;
    int64_t  step;          /* per iteration */
    uint64_t ind_start;     /* the iteration space: from ind_start, in steps, up to but not including ind_end */
    uint64_t ind_end;
    uint64_t loop_exit;     /* the rip every thread must exit at */
    uint64_t reserved[4];
    /* the reply */
    uint64_t exit_rip;
    uint64_t fault_addr;
    uint32_t exit_kind;     /* VPU_EXIT_* */
    uint32_t exit_thread;
    uint32_t chunks;        /* chunks mapped */
    uint32_t dirty;         /* pages written back */
    uint32_t faults;        /* signals taken */
    uint32_t threads_ran;
    uint64_t fetch_ns;      /* time waiting on the host for memory */
    uint64_t wb_ns;         /* time writing memory back */
    uint64_t run_ns;        /* between entry and the last exit */
    uint64_t reserved2[5];
    struct vpu_regs regs;   /* in: the state at entry; out: the state at the exit (64-byte aligned) */
    uint64_t code_page[VPU_EXEC_MAX_PAGES];
    struct vpu_range ranges[VPU_EXEC_MAX_RANGES];
};

/* The mailbox: the card writes addr, len, kind, then seq; the host serves
 * and writes status, then ack = seq. */
#define VPU_MAIL_FETCH      0   /* copy [addr, addr+len) of the program into fetch slot `slot`; unmapped parts as zero; acked when copied */
#define VPU_MAIL_WRITEBACK  1   /* apply write-back slot `slot`: len entries of struct vpu_wb_page, then the pages; acked before it is applied (the next ack means it was) */
struct vpu_mail {
    uint64_t seq;
    uint64_t addr;
    uint64_t len;
    uint32_t kind;
    uint32_t slot;          /* which of the two fetch or write-back slots */
    uint64_t ack;
    int32_t  status;        /* 0, or -1 when nothing of the range is mapped on the host */
    uint32_t pad2;
};

/* A written-back page: only the 64-byte lines whose bit is set in `lines`
 * changed on the card, and only those are written into the program. The
 * table of VPU_WB_TABLE bytes comes first in the slot, the pages after
 * it in table order. */
struct vpu_wb_page {
    uint64_t addr;      /* program address of the page */
    uint64_t lines;     /* bit n: line n of the page changed */
};
#define VPU_WB_TABLE        (64u << 10)
#define VPU_WB_MAX_PAGES    ((VPU_EXEC_CHUNK - VPU_WB_TABLE) / 4096u)

_Static_assert(sizeof(struct vpu_regs) == 2256, "register file layout is shared with Rust");
_Static_assert(sizeof(struct vpu_range) == 24, "range layout is shared with Rust");
_Static_assert(sizeof(struct vpu_mail) == 48, "mailbox layout is shared with Rust");
_Static_assert(sizeof(struct vpu_exec) == 256 + 2256 + 512 + 1536, "exec descriptor layout is shared with Rust");
_Static_assert(VPU_OFF_EXEC + sizeof(struct vpu_exec) <= 16384, "the descriptor stays in the control pages");

/* The card side (vpu_exec.c). */
int vpu_exec_run(volatile unsigned char *ctrl, int blk_fd, int verbose);
int vpu_exec_init(void);
/* From vpu_worker.c: run fn(arg, slice, nslices) on nslices threads of the
 * pool (the caller's thread is the last slice) and wait for all. */
int vpu_pool_map(void (*fn)(void *arg, int slice, int nslices), void *arg, int nslices);
int vpu_pool_threads(void);

#endif
