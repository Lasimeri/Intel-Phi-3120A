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

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* KNC_H */
