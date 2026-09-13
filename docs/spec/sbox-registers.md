# SBOX and DBOX registers used by this project

All offsets are relative to MMIO BAR4. SBOX registers are at
`0x10000 + offset`; the constant `SBOX_BASE = 0x10000` is `MIC_X100_SBOX_BASE_ADDRESS`
in mainline v5.9 `mic_x100.h`. The DBOX occupies BAR4 offsets 0 to 0xFFFF.
The authoritative Rust encoding is `host/crates/phi-regs/src/sbox.rs`.

| Name | SBOX offset | Width | Meaning | Source |
| --- | --- | --- | --- | --- |
| `SPAD0..SPAD15` | 0xAB20 + 4n | 32 | Scratchpad registers shared by host and card | mic_x100.h `MIC_X100_SBOX_SPAD0` |
| `SPAD2` (DOWNLOAD_INFO) | 0xAB28 | 32 | bit 0: firmware ready; bits 9:1: BSP APIC ID; bits 31:12: download address | mic_x100.h `MIC_X100_SPAD2_*` |
| `SPAD5` (FW_SIZE) | 0xAB34 | 32 | Size in bytes of the downloaded image | mic_x100.c `mic_x100_load_firmware` |
| `APICICR0..7` | 0xA9D0 + 8n | 2 x 32 | SBOX LAPIC interrupt command registers; low dword at +0 (vector, bit 13 send), high dword at +4 (destination APIC ID) | mic_x100.h, mic_x100.c `mic_x100_send_firmware_intr`; SSDG 4.2.4 |
| `APICICR7` | 0xAA08 | | The ICR Intel used for the boot IPI (vector 229) | mic_x100.h |
| `RGCR` | 0x4010 | 32 | Reset: set bit 0 to reset the card to the bootstrap | mic_x100.c `mic_x100_hw_reset` |
| `SICR0` | 0x9004 | 32 | System interrupt cause: bits 3:0 doorbells, 15:8 DMA | mic_x100.h |
| `SICE0` | 0x900C | 32 | System interrupt enable, same layout | mic_x100.h |
| `SICC0` | 0x9010 | 32 | System interrupt clear | mic_x100.h |
| `SIAC0` | 0x9014 | 32 | Auto-clear enable (MSI-X only) | mic_x100.c `mic_x100_enable_interrupts` |
| `MXAR0` | 0x9044 | 32 | MSI-X address/routing registers, 32 of them | mic_x100.h |
| `MSIXPBACR` | 0x9084 | 32 | MSI-X pending bit array control | mic_x100.h |
| `SMPT00..31` | 0x3100 + 4n | 32 | System memory page table: bits 31:2 host address >> 34, bit 0 no-snoop | intelmic.c, mic_x100.c `mic_x100_smpt_hw_init` |
| `RDMASR0..7` | 0xB180 + 4n | 32 | Card to host doorbell registers | mic_x100.h |
| `SDBIC0` | 0xCC90 | 32 | System doorbell interrupt control | mic_x100.h |

| Name | BAR4 offset (DBOX side) | Width | Meaning | Source |
| --- | --- | --- | --- | --- |
| `POSTCODE` | 0x242C | 32 | Bootstrap and OS progress code; 0x12 = waiting for OS download | mic_x100.h `MIC_X100_POSTCODE`, read via `mic_mmio_read(&mdev->mmio, ...)` without the SBOX base |

Registers not yet used (DMA engine, thermal, power, GDDR configuration) are
extracted from `micsboxdefine.h` in `vendor/mpss-3.8.6` when needed, and
added here with the same citation discipline.
