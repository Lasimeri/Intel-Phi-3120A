# Boot protocol: bootstrap expectations and the host loader sequence

Sources: SSDG 328207-002 sections 2.2.3 and 2.2.4; mainline Linux v5.9
`drivers/misc/mic/host/mic_x100.c` and `mic_x100.h` (functions named below);
Intel MPSS 2.1 readme (POST code list); Linux `Documentation/x86/boot.rst`.

## The bootstrap (card side, in flash)

`fboot0` (ROM) authenticates `fboot1` (flash). `fboot1` initializes the cores,
GDDR5 (with cached training parameters), the uncore, boots the APs to 64-bit
mode and parks them, then reaches POST code 0x12 and waits. On the download
interrupt it authenticates the image; an unsigned image is a "3rd party OS"
and is booted via the Linux 32-bit entry point after:

1. locking sensitive registers,
2. creating the `boot_params` structure,
3. building SFI tables (CPU list, memory map),
4. switching to 32-bit protected mode with paging off, `%esi` = boot_params,
   `KEEP_SEGMENTS` set because the bootstrap's GDT layout differs.

Known POST codes (MPSS 2.1 readme; the full table is in `mpss-boot-files`
documentation inside `vendor/`):

| Code | Meaning |
| --- | --- |
| 0x01 | LIDT |
| 0x02 | SBOX initialization |
| 0x03 | Set GDDR top |
| 0x05 | Program E820 table |
| 0x06 | Initialize DBOX |
| 0x0E | Copy AP boot code to GDDR |
| 0x12 | Waiting for coprocessor OS download (the "ready" state) |

The register is read as a 32-bit value at MMIO BAR offset 0x242c
(`MIC_X100_POSTCODE`). Its low two bytes are two ASCII characters, low
byte first (`"12"` reads as `0x3231`); the full table is in
`phi-regs/src/postcode.rs`. Measured 2026-09-13: `0x6330` = `"0c"` while
`SPAD2` reported ready (`docs/results/2026-09-13-first-contact.md`).
Values written by the running kernel (after the bootstrap) are the
kernel business; Intel kernel wrote progress codes there too, and this
project kernel will write its own.

## Host loader sequence (what Intel's driver did)

Register names are SBOX offsets relative to BAR4 + 0x10000 unless noted.
`SPADn` is `0xAB20 + 4*n`.

| Step | Action | Source |
| --- | --- | --- |
| 1 | Read `SPAD2`. Bit 0 = firmware ready (bootstrap waiting). Bits 9:1 = BSP APIC ID. Bits 31:12 = download address in card physical memory (`bootaddr`, typically 64 MiB). Reject if `bootaddr > 2^31`. | `mic_x100_get_boot_addr`, `mic_x100_is_fw_ready`, `mic_x100_get_apic_id` |
| 2 | Copy the bzImage to BAR0 offset `bootaddr`. | `mic_x100_load_firmware` |
| 3 | Write `SPAD5` = image size in bytes. | same (`MIC_X100_FW_SIZE = 5`) |
| 4 | Write the kernel command line, NUL-terminated, at BAR0 offset `bootaddr + image_size`. Intel prefixed ` mem=<aperture MiB>M`. | `mic_x100_load_command_line` |
| 5 | Copy the initramfs to BAR0 offset `2 * bootaddr` (128 MiB when bootaddr is 64 MiB). | `mic_x100_load_ramdisk` |
| 6 | In the bzImage copy in card memory, write `hdr.ramdisk_image = 2*bootaddr` (offset 0x218) and `hdr.ramdisk_size` (offset 0x21C). The bootstrap copies these into its own `boot_params`. | same |
| 7 | Write the BSP APIC ID to `APICICR7 + 4` (0xAA0C), read back to order the posted write, then write `vector 229 | (1 << 13)` to `APICICR7` (0xAA08). Bit 13 is the "send" bit. | `mic_x100_send_firmware_intr`, `MIC_X100_BSP_INTERRUPT_VECTOR = 229` |
| 8 | Poll: the card kernel comes up; Intel's stack considered the card online when the card's SCIF driver completed a handshake. This project polls the ring header magic instead. | cosm driver; this project: `phi-ring` |

Reset: read `RGCR` (0x4010), OR in bit 0, write it back, then sleep at least
one second before touching the card again (`mic_x100_hw_reset`). Afterwards
clear `SPAD2` to 0 and wait for the bootstrap to set bit 0 again
(`mic_x100_reset_fw_ready`, `mic_x100_is_fw_ready`).

## Inferences to verify in phase P1

- Whether the bootstrap reads `cmd_line_ptr` from the image header or derives
  the command line address from `bootaddr + SPAD5`. Intel's driver only wrote
  `SPAD5` and the bytes; it did not patch `cmd_line_ptr`. First boot of this
  project's kernel dumps `boot_params` to the console ring to settle it.
- Whether the SFI memory map or an e820 in `boot_params` (POST code 0x05
  says "Program E820 table") carries the memory layout. Both are handled.
- The exact `boot_params` contents the bootstrap fills (`hardware_subarch`,
  `setup_data`, `e820_entries`).

## Interrupts (for later phases)

- Card to host: the card writes `RDMASR0..7` (0xB180 + 4*n) or doorbell bits
  in `SICR0`; the host sees MSI-X vectors. Doorbell indexes 0..3, DMA 8..15,
  error 30 (`mic_x100.h`). Enable with `SICE0` (0x900C), auto-clear with
  `SIAC0` (0x9014) for MSI-X only.
- Host to card: write an `APICICRn` pair as in step 7 with the target APIC ID
  and a vector the card kernel registered.
