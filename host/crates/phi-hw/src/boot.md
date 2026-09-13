# phi-hw / boot.rs

The loader. Each numbered step in `boot()` corresponds to a row of the
table in `docs/research/boot-protocol.md`, which cites the v5.9 driver
function it was taken from.

## Additions beyond Intel's sequence

1. **Validation first.** The bzImage header is parsed, sizes are checked
   against the download address, the initramfs placement, the GDDR size,
   and the ring region, before any write. A bad file cannot half-load.
2. **Ring region formatting** before the boot interrupt, so the kernel's
   early console has a valid header from the first instruction. The region
   is formatted in host memory and copied with one bulk write.
3. **Command line parameters** `memmap=<size>K$<base>` (reserve) and
   `phi.ring=<base>,<size>` (locate) are appended unless `raw_cmdline`.
   Intel appended `mem=<aperture MiB>M`; this loader does not, because the
   aperture is 16 GiB and the card has 6 GB, so the bootstrap's memory map
   is the correct source.
4. **`cmd_line_ptr` and `type_of_loader`** are set in the image copy in
   addition to `SPAD5`, covering both possible bootstrap behaviors.
5. **Posted-write drain** by reading back the first bytes of the image
   before the interrupt, so the bootstrap cannot see the interrupt before
   the data.

## Initramfs placement

`2 * bootaddr`, which is 128 MiB when the bootstrap reports 64 MiB. That
leaves 64 MiB for the kernel image plus command line, far more than a
bzImage needs. If the initramfs is large (a full userland is 100 MB+), it
still fits below 6 GB; the check is explicit.

## Testing

The command-line helper is unit-tested. The rest runs only on hardware;
phase P1 records the first `BootReport` in `docs/results/`.
