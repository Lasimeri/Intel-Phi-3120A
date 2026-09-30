# card.S: the card as registers and memory

`card_open` requires a Phi bound to `vfio-pci`, then `vfio_open`.

**Registers.** `sbox_read`/`sbox_write` are single 32-bit accesses at
`SBOX_BASE + offset` in the BAR 4 mapping; `postcode` reads the raw BAR 4
offset 0x242c; `spad`/`set_spad` the scratchpads; `download_info` SPAD2
(bit 0 ready, bits 9:1 the BSP's APIC id, bits 31:12 the download
address: `mic_x100.h`). Uncached BAR mappings make every access a PCIe
transaction; nothing is cached or reordered by the CPU in a way the
device notices (the accesses are plain loads and stores to an uncached
mapping, which x86 issues in program order).

**wait_ready(ms)**: the bootstrap at POST "12" and SPAD2's ready bit
both, logging every POST change. SPAD2's bit survives the reset VFIO
issues at open and is stale until the bootstrap reaches "12" again; the
POST code is the authority (aperture writes on the stale bit, during
GDDR training, reset the host on 2026-09-14).

**card_reset(ms)**: `RGCR` bit 0, one second (Intel: "delay at least 1
second after touching reset"), SPAD2 cleared so a stale value cannot be
taken for the fresh announcement, then the ready bit awaited; POST
changes are logged with their time.

**send_icr / send_boot_interrupt**: the destination in the ICR's high
dword, read back to order the posted write, then the low dword with the
send bit (`SSDG 4.2.4`, `mic_x100_send_firmware_intr`): vector 229 to the
BSP through ICR 7.

**The aperture copies.** `aper_write` moves bytes into card memory with
single bytes up to 8-byte alignment, then 32-byte AVX2 stores
(`vmovdqu`) while 32 bytes remain, then 8-byte stores, then bytes.
`aper_read` mirrors it with 8-byte loads. The Rust `Mapping::write_bytes`
used 8-byte volatile stores; measured on card 1 (`phictl fill`, 16 MiB,
`docs/results/2026-09-29-phictl-assembly.md`): 0.56 to 0.58 s against
0.087 s with the read-back and 0.063 s without, 6.5x to 9x. A wider
store is one PCIe write transaction of 32 bytes where four were needed.
Both count in `traffic` (aperture to and from the card).

**write_card_memory** is the paced path: 4 KiB chunks, the last eight
bytes of each read back and compared. The non-posted read does not
complete until the card has accepted every preceding posted write, which
bounds the writes in flight toward a Gen2 endpoint; an unpaced 1 MiB
burst of 8-byte writes reset the host three times running on 2026-09-14.
`write_paced` exposes the chunk and the read-back flag for `phictl fill`.
Both refuse a range past the aperture; `write_card_memory` also past the
card's 6 GiB of GDDR.
