# phi-regs / bootparams.rs

The subset of the Linux x86 boot protocol the host loader needs.

## Why only the header

The on-card bootstrap is the "boot loader" in the protocol's sense: it
creates `struct boot_params` and enters the kernel at the 32-bit entry point
(SSDG 2.2.4). Intel's host driver therefore never built a zero page; it
copied the bzImage and patched two fields inside the image copy,
`ramdisk_image` and `ramdisk_size`, which the bootstrap evidently propagates
into its own `boot_params` (`mic_x100_load_ramdisk` in v5.9).

The loader in `phi-hw` does the same, after validating the image with
`parse_bzimage` so that a wrong file is rejected before anything touches
the card.

## Open item recorded in `docs/research/boot-protocol.md`

Whether the bootstrap derives the command line pointer from `SPAD5`
(image size) plus the download address, or reads `cmd_line_ptr` from the
header. Intel's driver wrote only `SPAD5` and the bytes. The loader
therefore writes `SPAD5` and the bytes and additionally sets
`cmd_line_ptr` in the image copy to the same address; setting both is
harmless and covers either behavior. Phase P3's early console dumps what
the kernel actually received.

## Testing

`cargo test -p phi-regs`: synthetic headers for a modern protocol version,
the "setup_sects 0 means 4" rule, and every rejection path.
