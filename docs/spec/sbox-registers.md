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

## Clock, voltage and sensors

| Name | SBOX offset | Width | Meaning | Source |
| --- | --- | --- | --- | --- |
| `SPAD4` (platform word) | 0xAB30 | 32 | bits 3:0 thread mask, 6:4 L2 size code, 9:6 memory channels minus one, 29:25 fused ICC divider (0 = nominal 20), bit 30 soft reset, bit 31 internal flash | k1om `intelmic.c` `sboxScratch4RegDef`; MPSS `ras/micras_knc.c` |
| `CURRENTRATIO` (`CURRENT_CLK_RATIO`) | 0x402C | 32 | bits 11:0 the PLL ratio the cores run at now, bits 27:16 the goal ratio; ratio = feedback (bits 8:1) over feed-forward code (bits 10:9: 3 = /1, 2 = /2, else /4) times the 4000 MHz reference over the ICC divider | k1om `mic_knc/micsboxdefine.h` `SBOX_CURRENTRATIO`, `intelmic.c` under `CONFIG_MK1OM`. **Not 0x3004**: that is the Knights Ferry offset, listed unguarded in the merged MPSS `micsboxdefine.h` |
| `COREFREQ` | 0x4100 | 32 | bits 11:0 the programmed ratio (same encoding), bit 15 fuse ratio, bit 16 async, bits 29:26 throttle step, bit 30 throttle jump, bit 31 booted | `micsboxdefine.h` under `CONFIG_MK1OM`; `mic_knc/micsboxstruct.h` |
| `COREVOLT` | 0x4104 | 32 | bits 7:0 VR12 SVID code: 250 mV plus 5 mV per step above code 1 | `micsboxdefine.h`; MPSS `ras/micras_knc.c` `svid2volt` |
| `THERMAL_STATUS` | 0x1018 | 32 | bit 31 valid, bits 30:22 TMU die temperature | `micsboxdefine.h`; `micras_knc.c` |
| `STATUS_FAN2` | 0x1028 | 32 | bits 19:12 VDDG regulator temperature | same |
| `BOARD_TEMP1`, `BOARD_TEMP2` | 0x1030, 0x1034 | 32 | air inlet and VCCP regulator; GDDR and GDDR regulator: 9-bit fields at 8:0 and 24:16 with valid bits 15 and 31 | same |
| `CURRENT_DIE_TEMP0..2`, `MAX_DIE_TEMP0..2` | 0x103C + 4n, 0x1048 + 4n | 32 | three 10-bit die temperatures per register (bits 9:0, 19:10, 29:20), nine sensors; the maxima are held since power-on | same |
| `ELAPSED_TIME_LOW/HIGH` | 0x1074, 0x1078 | 32 | elapsed time counter | `micsboxdefine.h` |

Measured on this card on 2026-09-17 (busybox `devmem` on the card at
physical `0x08007D0000 + offset`, `KNC_SBOX_PHYS`), kernel #38, idle:

| Register | Value | Decodes to |
| --- | --- | --- |
| SBOX+0x402C (`CURRENTRATIO`) | `0x04160416` | current and goal ratio `0x416`: feedback 11, feed-forward /2, 200 MHz reference (ICC divider 20 from `SPAD4` `0x2800e6cf`): 1100 MHz |
| SBOX+0x3004 (KNF offset) | `0x00000000` | nothing: not a register on KNC |
| SBOX+0x4100 (`COREFREQ`) | `0x80010416` | booted, fuse ratio, programmed ratio `0x416` |
| SBOX+0x4104 (`COREVOLT`) | `0x000000AB` | SVID `0xab`: 1100 mV |

## DMA engine

| Name | SBOX offset | Width | Meaning | Source |
| --- | --- | --- | --- | --- |
| `DCR` | 0xA280 | 32 | two bits per channel: bit 2n owner (1 host), bit 2n+1 enable | `micsboxdefine.h`; MPSS `dma/mic_dma_md.c` |
| `DCAR_n`, `DHPR_n`, `DTPR_n`, `DRAR_LO_n`, `DRAR_HI_n`, `DSTAT_n`, `DCHERR_n`, `DCHERRMSK_n` | 0xA000 + 0x40n + {0x00, 0x04, 0x08, 0x14, 0x18, 0x20, 0x2C, 0x30} | 32 | channel attributes (interrupt masks bits 24, 25), head and tail descriptor indices, ring address (low; high with size in bits 20:4, SMPT page 25:21, SYS bit 26), status, error, error mask | same; descriptor layouts in `include/mic/mic_dma_md.h` |

The Rust constants and decoders (`host/crates/phi-regs/src/sbox.rs`) and
the card kernel (`arch/x86/include/asm/knc.h`, `knc_hwmon.c`, patches
0010 and 0027) carry the same offsets; the crate's tests pin the decoders
to the values above.
