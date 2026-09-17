# phi-ring / region.rs

Formatting and opening a region. The host formats the region in card memory
*before* sending the boot interrupt, so the kernel's early console finds a
valid header from its first instruction.

## Layout algorithm

Header (64 bytes), then one 64-byte descriptor per channel, then the rings
in channel order, each `h2c` before `c2h`, each starting on a 64-byte
boundary, then every data area (channels that have one, in channel order)
page aligned after the last ring. `plan_layout` computes this without
writing, and `format` validates everything (ring sizes, the fit, the
32-bit header fields, the backing memory) before the first write. A failed
format leaves the old contents untouched. The region magic is cleared
first and rewritten last, with fences, so a reader that sees the magic sees
a complete table.

## Opening

`open` checks magic and version, then every field it is about to use: the
recorded region size against the backing memory (a `phictl` command run
with a different `--ring-size` than the boot reads a header that points
past its window), the channel count against the region, each ring's offset
(64-byte aligned, ring header plus data inside the region), each data area
(page aligned, inside the region). Descriptors of unknown kinds are skipped
without validation so an older host can open a region a newer host
formatted. Ring headers themselves are validated when an endpoint attaches.

## Card side

The card's drivers do the equivalent of `Region::open` with
`knc_ring_find` in `asm/knc_ring.h`: check the magic, walk at most eight
descriptors, attach to the ring of the kind they want after checking its
magic and size against the descriptor. The card is the producer on `c2h`
and the consumer on `h2c`.

## Default plan

`phi_hw::boot::DEFAULT_CHANNELS`: console (4 KiB host-to-card, 64 KiB
card-to-host, because boot logs are bursty), network (256 KiB each way),
rpc (256 KiB each way), block (16 KiB / 64 KiB plus an 8 MiB data area of
sixteen 512 KiB bounce slots) and host memory (16 KiB / 64 KiB). The layout
ends just above 9.2 MiB inside the 16 MiB default region; `phictl`'s disk
service uses the region's last 2 MiB (self-test scratch from 14 MiB, DMA
status words in the last page), which a test in `phictl` (`disk.rs`, `default_layout_leaves_the_self_test_area_clear`) keeps clear.

## Testing

Format/open round trips with and without data areas and the host memory
window, the plan computed without formatting, rejection of layouts that do
not fit, of unformatted memory and of every out-of-range header field.
