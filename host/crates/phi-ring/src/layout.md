# phi-ring / layout.rs

The wire format as offset constants. The card kernel reads the same
offsets from `arch/x86/include/asm/knc_ring.h` (kernel patches 0021, 0022,
0025, 0026), which is the authoritative card-side copy. The older C mirror
`card/drivers/phinet/include/phi_ring.h` and `tools/ring-layout-check.c`
cover only the original fields (kinds 1 and 2, no data area, no host memory
window) and are behind this file.

## Why offsets instead of structs

The bytes live in card memory reached through an uncached BAR. Building a
Rust struct over them would invite ordinary loads and stores (and the
compiler's freedom to merge them). Offsets plus `RingMemory` keep every
access explicit and volatile in the hardware backend.

## Cache-line discipline

`head` is written only by the producer, `tail` only by the consumer, and
each sits alone on a 64-byte line (`LINE`) so that neither side's writes
invalidate the other's line. The 64-byte figure is the KNC L1/L2 line size
(SSDG 328207-002, 2.1.1). Data areas (block bounce slots) are page aligned
(`DATA_ALIGN`) because the card hands them out as whole pages.

## Fields added after v1's first cut

Still version 1: every addition is a field that was zero padding before,
read only by a consumer that knows it exists.

| field | offset | added with |
| --- | --- | --- |
| `channel_desc.data_offset`, `data_size` | 24, 28 | block channel, kernel patch 0025 |
| `region_hdr.hostmem_addr`, `hostmem_size` | 32, 40 | host memory window, kernel patch 0026 |
| kinds 3 (rpc), 4 (block), 5 (host memory) | | patches 0022, 0025, 0026 |

## Testing

`constants_match_spec` pins the magics, the line alignment of every index
and the fact that the last header fields end inside their line;
`channel_kinds_round_trip_and_unknown_is_none` the wire values.
