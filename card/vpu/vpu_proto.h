/* The host/card contract for offloading AVX-512 work to the vector units.
 *
 * Both sides see the same physical host memory: the host as
 * /dev/shm/phi-hostmem, the card as /dev/phihost (mapped, uncached) and
 * as /dev/phiblk1 (the same bytes, reached through the DMA engine).
 * Verified on the card 2026-09-22: a marker written by the host at window
 * offset 1 MiB reads back identically through the block device at the
 * same offset.
 *
 * Measured on this hardware, which is what the split below is built on:
 *
 *   doorbell round trip, through the mapping    2.36 us
 *   bulk transfer, block device, 16 MiB reads   1.2 GB/s
 *   direct access to the mapping                50 MB/s
 *
 * So control words go through the mapping, where latency is what matters
 * and there are only a few bytes, and bulk data goes through the block
 * device, where bandwidth is what matters. Using the mapping for bulk
 * data would be twenty times slower.
 *
 * The Rust mirror of this file is host/crates/phi-vpu/src/proto.rs. Both
 * carry the same size assertions, and tools/vpu-layout-check.c compares
 * every offset between the two.
 */
#ifndef VPU_PROTO_H
#define VPU_PROTO_H
#include <stdint.h>

/* Control words, each on its own 64-byte line so the two sides never
 * share one. */
#define VPU_OFF_READY   0     /* card writes VPU_MAGIC when it is polling */
#define VPU_OFF_REQ    64     /* struct vpu_request */
#define VPU_OFF_REPLY 256     /* struct vpu_reply */
#define VPU_OFF_DATA  (1u << 20)   /* bulk data starts here, page aligned */

#define VPU_MAGIC 0x5650555F52454144ULL   /* "VPU_READ" */

/* Kernels the worker can run. Each is AVX-512 translated to this card's
 * own instruction set by host/crates/avx512-xlate. */
#define VPU_K_POLY30 1   /* degree-30 Horner, float32: out[i] = poly(in[i]) */
#define VPU_K_EXEC   2   /* run a region of the host program on the card: vpu_exec.h */

/* Alignment rules, all consequences of the card reading and writing the
 * window with O_DIRECT (the page cache would otherwise serve stale data):
 *
 *   - in_off, out_off and aux_off are multiples of VPU_BLOCK
 *   - the card transfers whole blocks: it reads and writes
 *     round_up(len, VPU_BLOCK) bytes at each offset, so every region must
 *     occupy whole blocks and nothing else may live in the slack
 *   - POLY30 works in steps of VPU_CHUNK elements; elements past n up to
 *     the next multiple of VPU_CHUNK are computed on whatever the slack
 *     holds and written back as garbage inside the output's last block
 */
#define VPU_BLOCK 4096u
#define VPU_CHUNK 128u

struct vpu_request {
    uint64_t seq;       /* host bumps this last; the card polls it */
    uint32_t kernel;    /* VPU_K_* */
    uint32_t threads;   /* how many card threads to spread across */
    uint64_t n;         /* elements */
    uint64_t in_off;    /* window offset of the input */
    uint64_t out_off;   /* window offset of the output */
    uint64_t aux_off;   /* window offset of coefficients, if any */
    uint64_t aux_len;
};

/* status values */
#define VPU_OK          0
#define VPU_E_ALLOC    -1   /* card could not reserve buffers */
#define VPU_E_PULL     -2   /* reading the input from the window failed */
#define VPU_E_PUSH     -3   /* writing the output failed (unaligned out_off?) */
#define VPU_E_REQUEST  -4   /* n is zero or an offset is not block aligned */
#define VPU_E_KERNEL   -5   /* unknown kernel number */

struct vpu_reply {
    uint64_t seq;        /* echoes the request when the work is done */
    uint64_t compute_ns; /* time on the vector units alone */
    uint64_t total_ns;   /* from doorbell seen to reply written */
    uint64_t pull_ns;    /* moving input and coefficients onto the card */
    uint64_t push_ns;    /* moving the output back */
    int32_t  status;     /* VPU_OK or a VPU_E_* value */
    int32_t  threads;    /* slices actually run */
};

_Static_assert(sizeof(struct vpu_request) == 56, "request layout is shared with Rust");
_Static_assert(sizeof(struct vpu_reply) == 48, "reply layout is shared with Rust");

#endif
