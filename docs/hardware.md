# Hardware

Every row names its source: an Intel document and section, a database, or
a measurement on this machine with its date and the record in
`docs/results/`. Rows without a primary source say so.

## The card

Intel Xeon Phi coprocessor **3120A** (Knights Corner, x100 family).
Identification on this machine, measured 2026-09-13 with `lspci -nn`:

```
2e:00.0 Co-processor [0b40]: Intel Corporation Xeon Phi coprocessor 3120 series [8086:225d] (rev 20)
        Subsystem: Intel Corporation Device 3c98
```

Subsystem `8086:3c98` decodes to "3120A/3140A" in the linux-hardware.org
database (`pci:8086-225d-8086-3c98`); the 3120P uses a different subsystem
ID.

| Property | Value | Source |
| --- | --- | --- |
| Cores / hardware threads | 57 cores, 4 threads per core, 228 threads | Intel ARK SKU 75797; measured: `SPAD4` thread mask `0xF`, 228 CPUs in the SFI CPU table, `nproc` = 228 (`results/2026-09-14-p4-tty-smp.md`) |
| Core clock | 1.10 GHz | Intel ARK; measured: `COREFREQ` `0x416` (feedback 11, feed-forward 2, 200 MHz reference from `SPAD4`) decodes to 1100 MHz; `CURRENTRATIO` (SBOX+0x402C, the Knights Corner offset) reads `0x04160416`, the same ratio; the Knights Ferry offset 0x3004 that the merged MPSS header lists reads 0, which is why the 2026-09-16 record says the register "reads 0" (`spec/sbox-registers.md`, measured 2026-09-17; kernel log `KNC: core clock 1100000 kHz`) |
| L2 | 512 KiB per core, 28.5 MiB total, coherent ring | Intel ARK; SSDG 328207-002, 2.1; measured: `SPAD4` cache-size field = 512 KiB |
| Vector unit | 512-bit VPU per core, 32 zmm registers, 8 mask registers, 16 SP / 8 DP lanes, FMA; MVEX-encoded, not AVX-512 | ISA reference 327364-001, chapters 3 and 6; measured 2026-09-15: a vector instruction cannot issue on consecutive cycles from one thread, so the unit needs two or more threads per core (`results/2026-09-15-vpu.md`) |
| Vector state and FXSAVE | FXSAVE skips the zmm registers, FXRSTOR clears their low 128 bits | ISA reference B.4 and B.5; measured: 128,214 of 623,255 register checks mismatched without kernel patch 0024, 0 with it (`results/2026-09-15-vpu.md`) |
| MXCSR | Bit 21 (DUE) must be set in every FXRSTOR image, else #GP | Intel's k1om kernel (`arch/x86/include/asm/i387.h`, "KNC errata"); measured 2026-09-14: the first user task died in `fxrstor64` with the mainline default `0x1f80` (`results/2026-09-14-p4-tty-smp.md`) |
| Memory | 6 GB GDDR5, 12 channels active on this SKU, 240 GB/s, ECC capable | Intel ARK (size, bandwidth, ECC); channel count measured: `SPAD4` channel field = 12; the card kernel sees 5669 MiB usable (`phitop`, 2026-09-17) |
| PCIe | Gen2 x16 capable | Datasheet 328209; measured: `max_link_speed` 5.0 GT/s, `max_link_width` x16 |
| TDP and power connectors | 300 W; one 6-pin and one 8-pin auxiliary connector, both required | Datasheet 328209, table 2-1; Intel ARK (TDP) |
| Cooling | Intel lists the 3120A as the actively cooled SKU (on-card blower) and the 3120P as passive | Intel ARK SKUs 75797 and 75798. **Unverified on this unit:** the records `results/2026-09-16-sensors.md` and `results/2026-09-17-phitop.md` describe the card in this machine as passively cooled on chassis airflow; this port reads no fan telemetry (the SMC is not driven), so the repository has no measurement either way |
| CPU family/model/stepping | family 0x0B, model 0x01, stepping 2 | ISA reference App. B, CPUID leaf 1; measured: kernel log `family 0xb model 0x1 stepping 0x2` (`results/2026-09-14-first-boot.md`) |
| ISA | x86-64 base minus a long list of instructions, plus the KNC vector ISA | ISA reference App. B; see `research/isa-deletions.md` |
| Bootstrap | POST `"12"` (ready) 9.3 s after an `RGCR` reset, of which 7.4 s is GDDR training; the download address is 64 MiB, the BSP is APIC ID 224 | measured 2026-09-13 (`results/2026-09-13-reset-2.md`) |
| Die temperatures | 47 to 55 C idle, 59 C after 20 s of full load, maximum ever recorded by the SBOX 66 C; sensors 8 and 9 read 0 (unfused) | measured 2026-09-16 and 2026-09-17 (`results/2026-09-16-sensors.md`, `phictl sensors`) |
| Core voltage | 1100 mV (SVID code `0xab`, VR12: 250 mV + 5 mV per step) | decoding from MPSS 3.8.6 `ras/micras_knc.c` (reference only); measured 2026-09-16 |

## The host

| Item | Value | Source |
| --- | --- | --- |
| CPU | AMD Ryzen 7 5800X, 8 cores, 16 threads | `lscpu`, 2026-09-13 |
| Chipset | AMD X570 (Matisse), CPU-direct PCIe x16 bifurcated x8/x8 | `lspci -tv` (bridges `00:03.1` and `00:03.2` under the CPU root complex), 2026-09-13 |
| RAM | 64 GiB | `free -g`, 2026-09-13 |
| Host kernel | 7.2.6-1-cachyos, Arch-based | `uname -r`, 2026-09-19. Was 7.2.3-1-cachyos from 2026-09-13 to 2026-09-17; every result before that date was taken on 7.2.3. Nothing in this project changed across the upgrade (ADR 0001). The `vfio-pci` binding did not survive the first reboot, because this host predated the `modprobe.d` file; with that file it does, observed 2026-09-19 (`scripts/setup-arch.md`) |
| IOMMU | AMD-Vi enabled, interrupt remapping enabled, DMA domain lazy TLB invalidation | kernel log (`AMD-Vi` lines), 2026-09-13 |
| Other GPU | NVIDIA RTX 3090 Ti on the sibling x8 bridge (`00:03.2`, bus `2f`) | `lspci`, 2026-09-13 |
| Disk for the card image | WD Black SN850X 1 TB NVMe, xfs, at `/mnt/1TB-NVMe` | `results/2026-09-16-storage.md` |

## Measured PCIe state (2026-09-13)

| Item | Value | Note |
| --- | --- | --- |
| Link | Gen2 (5 GT/s) x8 | Card caps Gen2 x16. Parent bridge `00:03.1` caps x8. Slot-limited; no training fault. |
| BAR0 (MEMBAR0, aperture) | `0x7c00000000`, 16 GiB, 64-bit prefetchable | Above-4G decoding works. Card GDDR is 6 GB; the aperture is oversized by design. |
| BAR4 (MEMBAR1, MMIO) | `0xfce00000`, 128 KiB, 64-bit | DBOX registers in the first 64 KiB, SBOX in the second (SSDG 2.1.12; `spec/sbox-registers.md`). |
| Command register | `0x0000` before any driver | Memory decode and bus master are enabled by `vfio-pci` at open (`results/2026-09-13-first-contact.md` shows `0x0006`). |
| AER counters | all zero | After the host resets of 2026-09-13 the card showed sticky `DevSta: CorrErr+ UnsupReq+` (`results/2026-09-13-p3-kernel-build.md`). |
| IOMMU group | 30, contains only the card | VFIO passthrough needs no ACS override. |
| Reserved IOVA in group 30 | `0xfee00000-0xfeefffff` (MSI), `0xfd00000000-0xffffffffff` | The host's DMA mappings avoid both (`host/crates/phi-hw/src/dma.md`). |
| Interrupt pin | B, unrouted | Expected without a driver; nothing uses interrupts yet. |

Commands used: `lspci -nn`, `lspci -vvv -s 2e:00.0`,
`cat /sys/bus/pci/devices/0000:2e:00.0/{current_link_speed,current_link_width,resource}`,
`cat /sys/kernel/iommu_groups/30/reserved_regions`, `setpci -s 2e:00.0 COMMAND`;
`scripts/verify-card.sh` prints the same set.

## Measured transfer rates

| Path | Rate | Record |
| --- | --- | --- |
| DMA engine, card memory to host memory, 2 to 4 MiB copies in the self-test | 3.58 GB/s with tail-pointer completion, 2.3 to 2.5 GB/s with status-descriptor completion (the correct one) | `results/2026-09-16-dma.md` |
| `/dev/phiblk0` raw reads, 1 MiB requests, DMA direct mode | 846 MB/s; 370 MB/s at 256 KiB, 124 MB/s at 64 KiB | same |
| ext4 on `/dev/phiblk0` | 159 MB/s write with `fsync`, 348 MB/s read after a cache drop | same |
| Aperture copies (no DMA engine) | about 14 MB/s for the disk; 6 to 9 MB/s for `phictl put` | `results/2026-09-16-dma.md`, `2026-09-14-p5-net-rpc.md` |
| SSH through the userspace forwarder | about 0.6 MB/s (`scp`) | `results/2026-09-14-p5-net-rpc.md`, test 16 |
| Ping over `phi0` (two 1 kHz polls) | 2.6 ms | same file |

## Power

300 W for the card plus a 3090 Ti at stock limits exceeds the 900 W UPS on this
host (operational fact of this machine, no document). Rule: power-limit the
GPU (`nvidia-smi -pl`) or keep the card idle during GPU work. Card idle power
with the cores clock-gated is low (SSDG 2.1.13), but a booted kernel that
spins 228 threads is not idle. Power telemetry lives in the SMC, which this
port does not drive, so no card power figure has been measured.

## Why x8 is the ceiling

The Matisse x16 slot is split x8/x8 across bridges `00:03.1` (Phi, bus `2e`)
and `00:03.2` (3090 Ti, bus `2f`). Moving the Phi to full x16 would push the
GPU onto the chipset's Gen4 x4 link. Gen2 x8 is 4 GB/s per direction raw
(5 GT/s, 8b/10b, 8 lanes), and the DMA engine measured 3.58 GB/s memory to
memory, so the link, not the engine, is the bound.
