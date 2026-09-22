/* vpu_exec.h: the seamless path's contract between libphi512 on the host
 * and the card worker. The host hands the card a region of the program's
 * own machine code (its AVX-512 instructions rewritten to the card's
 * MVEX encoding in place, thunks for the few that need a sequence, and
 * ud2 at every exit), the full register file, and nothing else. The card
 * maps the program's memory at the program's own addresses, a chunk at a
 * time as the code touches it, fetched from the host through a mailbox;
 * runs the region on its vector units; and returns the register file,
 * where the region left off, and every chunk it wrote. See vpu_exec.md.
 *
 * Offsets are in the shared window (host: /dev/shm/phi-hostmem[-N];
 * card: /dev/phihost, and /dev/phiblk1 for bulk). Everything the card
 * reads in bulk is VPU_BLOCK aligned. Mirrored in
 * host/crates/phi-vpu/src/proto.rs; tools/vpu-layout-check.c checks. */
#ifndef VPU_EXEC_H
#define VPU_EXEC_H
#include <stdint.h>

#define VPU_EXEC_CHUNK      (2u << 20)     /* the unit of the program's memory moved either way: one huge page, so one DMA record per 512 KiB */
#define VPU_EXEC_THUNK_MAX  (64u << 10)    /* thunk area: out-of-line sequences and the entry stub */

#define VPU_OFF_MAIL        4096           /* struct vpu_mail: the card asks, the host answers */
#define VPU_OFF_EXEC        8192           /* struct vpu_exec: the region and the register file */
#define VPU_OFF_EXEC_FETCH  (32u << 20)    /* a chunk the host copied for the card */
#define VPU_OFF_EXEC_CODE   (VPU_OFF_EXEC_FETCH + VPU_EXEC_CHUNK)   /* the code chunk as the host prepared it */
#define VPU_OFF_EXEC_THUNK  (VPU_OFF_EXEC_CODE + VPU_EXEC_CHUNK)    /* the thunk area as the host prepared it */
#define VPU_OFF_EXEC_WB     (VPU_OFF_EXEC_THUNK + VPU_EXEC_CHUNK)   /* a chunk the card wrote, for the host to copy back */

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

/* Why the region stopped. */
#define VPU_EXIT_LEFT       0   /* control left the region (its ud2, or a jump elsewhere): rip is where the host resumes */
#define VPU_EXIT_FAULT      1   /* a data access the host could not serve: fault_addr, rip */
#define VPU_EXIT_ILLEGAL    2   /* an instruction inside the region the card refused: rip */
#define VPU_EXIT_COLLISION  3   /* a program address is already in use on the card: fault_addr */
#define VPU_EXIT_LIMIT      4   /* more chunks than the card tracks */

struct vpu_exec {
    uint64_t code_addr;     /* program address of the code chunk (chunk aligned); its bytes at VPU_OFF_EXEC_CODE */
    uint64_t code_len;      /* bytes, a multiple of VPU_BLOCK, at most VPU_EXEC_CHUNK */
    uint64_t thunk_addr;    /* program address the thunk area is mapped at (page aligned; free on the host and the card) */
    uint64_t thunk_len;     /* bytes, a multiple of VPU_BLOCK, at most VPU_EXEC_THUNK_MAX */
    uint64_t region_lo;     /* the region: rip in [lo, hi) or in the thunk area is inside; anywhere else is an exit */
    uint64_t region_hi;
    uint64_t entry;         /* where to start: an address in the thunk area (the entry stub loads rax and jumps) */
    uint64_t reserved[9];
    /* the reply */
    uint64_t exit_rip;
    uint64_t fault_addr;
    uint32_t exit_kind;     /* VPU_EXIT_* */
    uint32_t chunks;        /* chunks fetched */
    uint32_t dirty;         /* chunks written back */
    uint32_t faults;        /* signals taken */
    uint64_t fetch_ns;      /* time waiting on the host for chunks */
    uint64_t wb_ns;         /* time writing chunks back */
    uint64_t run_ns;        /* everything between entry and exit */
    uint64_t reserved2[9];
    struct vpu_regs regs;   /* in: the state at the fault; out: the state at the exit (64-byte aligned) */
};

/* The mailbox: the card writes addr, len, kind, then seq; the host serves
 * and writes status, then ack = seq. */
#define VPU_MAIL_FETCH      0   /* copy [addr, addr+len) of the program into VPU_OFF_EXEC_FETCH; unmapped parts as zero */
#define VPU_MAIL_WRITEBACK  1   /* apply the pages at VPU_OFF_EXEC_WB: len entries of struct vpu_wb_page, then the pages */

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
struct vpu_mail {
    uint64_t seq;
    uint64_t addr;
    uint64_t len;
    uint32_t kind;
    uint32_t pad;
    uint64_t ack;
    int32_t  status;        /* 0, or -1 when nothing of the range is mapped on the host */
    uint32_t pad2;
};

_Static_assert(sizeof(struct vpu_regs) == 2048 + 16 + 48 + 128 + 16, "register file layout is shared with Rust");
_Static_assert(sizeof(struct vpu_mail) == 48, "mailbox layout is shared with Rust");
_Static_assert(sizeof(struct vpu_exec) == 256 + 2256, "exec descriptor layout is shared with Rust");
_Static_assert(VPU_OFF_EXEC + sizeof(struct vpu_exec) <= 16384, "the descriptor stays in the control pages");

/* The card side (vpu_exec.c): run one request; the descriptor is read
 * from and written back to the window. Returns a VPU_E_* status. */
int vpu_exec_run(volatile unsigned char *ctrl, int blk_fd, int verbose);
int vpu_exec_init(void);

#endif
