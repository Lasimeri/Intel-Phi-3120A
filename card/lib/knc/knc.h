/* knc.h: the card's 512-bit vector unit, as a C ABI.
 *
 * Knights Corner vector instructions use the MVEX prefix, which no
 * assembler in this stack and no compiler anywhere still emits. Every
 * kernel behind this header is therefore hand-encoded byte by byte by
 * host/crates/knc-mvex and assembled as .byte directives; nothing here is
 * auto-vectorised and nothing here can be. See knc.md.
 *
 *   cc -O2 prog.c -lknc        on the card
 *   knc-cc -O2 prog.c -lknc    on the host, for the card
 *
 * C++ callers may include this directly, or knc.hpp for the typed layer.
 */
#ifndef KNC_H
#define KNC_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---------------------------------------------------------------- copy */

/* Copy `blocks` whole 64-byte blocks. Both pointers must be 64-byte
 * aligned. Returns dst, like memcpy.
 *
 * Measured 2026-09-20 against musl's memcpy on the card: 1.63x at 64
 * bytes, 1.11x at 1 MiB where both are limited by memory rather than by
 * instructions (card/examples/vpu_memcpy.md). Use it for short aligned
 * copies; for long ones it barely matters. */
void *knc_memcpy64(void *dst, const void *src, size_t blocks);

/* ---------------------------------------------------------- bit packing */

/* Values in one packed block. Fixed: the layout is built around sixteen
 * 32-bit lanes carrying 64 values each. */
#define KNC_BLOCK 1024

/* Bytes one packed block occupies at `bits` bits per value. */
#define KNC_PACKED_BYTES(bits) ((size_t)KNC_BLOCK * (bits) / 8)

/* Unpack one block of KNC_BLOCK values of `bits` bits into `out`.
 * Pack one block of KNC_BLOCK values into `packed`.
 *
 * `bits` runs from 1 to 32. `out` holds KNC_BLOCK int32; `packed` holds
 * KNC_PACKED_BYTES(bits) bytes. Both pointers must be 64-byte aligned.
 * Packing masks each value to `bits` bits, so a value that does not fit
 * is truncated rather than corrupting its neighbours. A `bits` outside 1
 * to 32 returns without touching memory: the dispatch is a computed jump
 * and this library is called from languages where the width can come from
 * data.
 *
 * The layout is not a contiguous bitstream. Value `i` lives in lane
 * `i % 16` at position `i / 16`, so all sixteen lanes sit at the same bit
 * offset at the same time and one shift-and-mask pair serves all of them.
 * This is the FastLanes unified layout at a 512-bit register width; it is
 * what makes bit-packing vectorisable at all, and it is not interchangeable
 * with an ordinary packed bitstream. knc.md has the details. */
void knc_unpack(int *out, const void *packed, unsigned bits);
void knc_pack(void *packed, const int *values, unsigned bits);

/* --------------------------------------------------- cascaded encodings */

/* Lanes in the layout. A base vector below is one value per lane. */
#define KNC_LANES 16

/* Bit packing alone is the bottom layer: columnar data is rarely small
 * because its values are small, it is small because they are close to a
 * common base (frame of reference) or to their neighbours (delta). Both
 * are the same instruction, `vpaddd`, on the same layout.
 *
 * Decoding folds the transform into the unpack kernel, so it costs one
 * instruction per sixteen values:
 *
 *   knc_unpack_for    out[i] = unpacked[i] + base[i % 16]
 *   knc_unpack_delta  out[i] = the running sum along positions within
 *                              each lane, starting from base[i % 16]
 *
 * `base` is KNC_LANES int32, 64-byte aligned. For frame of reference it
 * is the reference value per lane (fill all sixteen with one number for a
 * scalar frame). For delta it is the value that came before position 0 in
 * each lane, which is the previous block's last value in that lane.
 *
 * Note the stride. Value `i` lives in lane `i % 16`, so for delta "the
 * value before it" is the value sixteen positions earlier in `values`, not
 * its neighbour. On a sorted column that costs three to four bits per
 * value against a layout that could give adjacent differences;
 * `tools/delta-stride.md` measures it and says why this layout is still
 * the right one here.
 *
 * **Both require the stored residue to fit in `bits` bits, unsigned.**
 * `value - base` for frame of reference, and each difference for delta,
 * must lie in [0, 2^bits). Delta therefore wants ascending data; a column
 * that goes down as well as up needs a zigzag transform first, which this
 * library does not provide. Out of range, packing truncates and decoding
 * returns a different number rather than failing. */
void knc_unpack_for(int *out, const void *packed, unsigned bits, const int *base);
void knc_unpack_delta(int *out, const void *packed, unsigned bits, const int *base);

/* The encode side is a separate pass rather than being folded into the
 * packer, because a value that straddles two packed words is read twice
 * and would be transformed twice. One pass costs one instruction per
 * sixteen values, and cannot get that wrong. Feed the output to knc_pack.
 *
 *   knc_encode_for    out[i] = values[i] - base[i % 16]
 *   knc_encode_delta  out[i] = values[i] - the value before it in its lane
 */
void knc_encode_for(int *out, const int *values, const int *base);
void knc_encode_delta(int *out, const int *values, const int *base);

/* The per-width kernels, if a caller wants to hoist the dispatch out of a
 * loop. Indexed by bit width; entry 0 is null. */
typedef void (*knc_unpack_fn)(int *out, const unsigned *packed);
typedef void (*knc_pack_fn)(unsigned *packed, const int *values);
typedef void (*knc_unpack_base_fn)(int *out, const unsigned *packed, const int *base);

extern const knc_unpack_fn knc_unpack_table[33];
extern const knc_pack_fn knc_pack_table[33];
extern const knc_unpack_base_fn knc_unpack_for_table[33];
extern const knc_unpack_base_fn knc_unpack_delta_table[33];


/* ---- The FastLanes layout -----------------------------------------
 *
 * Everything above uses 16 lanes, which is the card's vector width.
 * FastLanes (github.com/cwida/FastLanes) uses 32 lanes for 32-bit values:
 * its generated `unffor_NNbw_32ow_32crw_1uf` reads `in + w*32 + i` and
 * writes `out + p*32 + i` for `i` in 0..32. A lane row is therefore 128
 * bytes, or two vectors, and lanes 0 to 15 and 16 to 31 never interact.
 *
 * `knc_fls_unffor` is that layout's decode step: unpack `bw`-bit values
 * and add the frame of reference, which FastLanes stores as a single
 * scalar and broadcasts. It exists so an unmodified FastLanes build can
 * route its 32-bit hot path here; see card/lib/knc-fls/knc-fls.md.
 *
 * Three constraints, all different from the functions above:
 *
 *   - `bw` runs 0 to 32, and 0 is a real width: every value is the base.
 *   - `base` points at one 32-bit value and **must be 4-byte aligned**.
 *     The kernels splat it with `vpbroadcastd`, which is `#GP` on any
 *     other address (ISA reference 327364-001, VPBROADCASTD,
 *     "Exceptions"). FastLanes' own base pointer is not: a base segment
 *     holds four bytes per vector but starts at an arbitrary byte offset
 *     in the file, so the caller copies the value through a local. The
 *     scalar code it replaces reads it with a plain load and never
 *     noticed.
 *   - `in` need only be 4-byte aligned, because FastLanes' bitpacked
 *     segments start at an arbitrary multiple of four inside its file
 *     buffer. The kernels use the unaligned load pair for this.
 *   - The load pair reads up to 60 bytes past `bw * 128`, because it
 *     always touches the whole of both 64-byte lines around an address
 *     (ISA reference 327364-001, VLOADUNPACKLD: "the memory region
 *     accessed will always be between linear_address & (~0x3F) and
 *     (linear_address & (~0x3F)) + 63"). This needs no padding from the
 *     caller: those bytes share a 64-byte line with the last byte of the
 *     input, a line never crosses a page, so the read cannot reach an
 *     unmapped one. The remaining case, an address that is itself
 *     64-byte aligned, the same section exempts from #PF outright.
 *
 * `out` must still be 64-byte aligned; FastLanes' own buffers are
 * declared `alignas(64)`, and the caller is responsible for checking it.
 *
 * Only 32-bit values are provided, and that is the hardware, not a
 * choice: Knights Corner has no byte or word integer vector instruction,
 * and its whole 64-bit integer vector set is vfixupnanpd, vpandnq,
 * vpandq, vpblendmq, vporq and vpxorq (ISA reference 327364-001,
 * appendix D.1.8), with no add and no shift. */
#define KNC_FLS_LANES 32
#define KNC_FLS_BLOCK 1024
#define KNC_FLS_PACKED_BYTES(bw) ((size_t)(bw) * 128u)

void knc_fls_unffor(const unsigned *in, unsigned *out, unsigned bw, const unsigned *base);

typedef void (*knc_fls_unffor_fn)(const unsigned *in, unsigned *out, const unsigned *base);

extern const knc_fls_unffor_fn knc_fls_unffor_table[33];

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* KNC_H */
