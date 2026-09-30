# boot.S: `phictl boot`

The image loader (`docs/research/boot-protocol.md`,
`host/crates/phi-hw/src/boot.rs`) and the command's options, then the
services (`console.S`).

Options as the Rust command had them: `--kernel`, `--initrd`,
`--cmdline` (default `earlyprintk=phiring,keep loglevel=8`),
`--ring-base` (256 MiB), `--ring-size` (16 MiB), `--raw-cmdline`,
`--no-console`, `--load-only`, `--watch ADDR`, `--net NAME`, `--net-addr
A.B.C.D/24` (default `10.9.N.1/24` for card N), `--forward HOST:CARD`,
`--serve [PATH]` (`auto` without a value), `--disk IMG`, `--no-dma`,
`--host-mem SIZE`, `--owner UID`.

The sequence, after opening the card and mapping the images (the kernel
as a private copy-on-write map so its header can be patched):

1. Host memory (`--host-mem`): the shared file `/dev/shm/phi-hostmem[-N]`
   sized, mapped populated, pinned for the card at IOVA 4 GiB, its card
   address `0x81_0000_0000` announced in the region header before the
   boot so the card's `/dev/phihost` and `/dev/phiblk1` find it.
2. The bootstrap at POST "12" (`wait_ready`, 20 s); the download address
   from SPAD2, plausible (non-zero, at most 2 GiB).
3. The bzImage: length, the 0xAA55 boot flag, `HdrS`, protocol at least
   2.03; `cmdline_size` (255 before 2.06).
4. The command line: the option's, trimmed, plus (unless raw)
   `memmap=<size>K$<base> phi.ring=<base>,<size> mem=6144M`, which
   reserves the region, tells the platform code where it is, and caps
   the kernel at the card's GDDR so nothing the bootstrap lists above it
   (registers, the SMPT window) is treated as RAM. Refused past
   `cmdline_size`.
5. Placement: the command line right after the image; the initramfs at
   twice the download address (Intel's choice), checked clear of the
   command line's end and inside GDDR; the ring region checked against
   the download address and the initramfs.
6. The region formatted in host memory (`ring.S`) and copied in one go;
   the kernel with `ramdisk_image`, `ramdisk_size`, `cmd_line_ptr` and
   `type_of_loader` 0xFF patched in the private copy; SPAD5 = the image
   size; the command line NUL-terminated; the initramfs. Every copy is
   the paced aperture path (`card.md`).
7. The boot interrupt (vector 229 to the BSP through ICR 7), unless
   `--load-only`. No read-back first: the interrupt is a register write
   the card orders after the posted aperture writes.
8. SMPT entries 0 to 3 identity, so the announced window is valid before
   the card touches it.

The report is the Rust command's, line for line (`boot interrupt sent to
APIC 224 after 9.31s`, image, cmdline, initramfs, ring). Then the daemon
loop unless `--no-console` or `--load-only`.

Verified on card 1 against the Rust daemon's journal for the same
images: identical addresses and sizes; the card reached K7 with 228 CPUs
(`docs/results/2026-09-29-phictl-assembly.md`).
