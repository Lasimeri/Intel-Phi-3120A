# phi-ring / region.rs

Formatting and opening a region. The host formats the region in card memory
*before* sending the boot interrupt, so the kernel's early console finds a
valid header from its first instruction.

## Layout algorithm

Header (64 bytes), then one 64-byte descriptor per channel, then the rings
in channel order, each `h2c` before `c2h`, each starting on a 64-byte
boundary. Sizes are validated before anything is written so a failed format
leaves the old contents untouched except for the magic, which is cleared
first and rewritten last. A reader that sees the magic therefore sees a
complete table.

## Card side

The card's `phinet` driver and the early console do the equivalent of
`Region::open`: check magic and version, walk the descriptor table, attach
to the ring of the kind they want. The card is the producer on `c2h` and
the consumer on `h2c`.

## Default plan

`phictl boot` formats one console channel (4 KiB host-to-card, 64 KiB
card-to-host, because boot logs are bursty) and one network channel
(256 KiB each way). The whole thing is under 600 KiB, inside the 1 MiB
region reserved by `memmap=`.
