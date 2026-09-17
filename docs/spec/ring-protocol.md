# Ring protocol v1: host/card shared-memory transport

Status: specified and implemented. Host: `host/crates/phi-ring` (layout),
`phictl console` (console channel), `phictl boot --net` (network channel,
`host/crates/phictl/src/net.rs`). Card: kernel patches 0011/0016 (console)
and 0021 (`arch/x86/kernel/knc_net.c`, network), sharing
`arch/x86/include/asm/knc_ring.h`.

## Placement

A contiguous region of card GDDR, `PHI_RING_REGION_SIZE` bytes (16 MiB by default since 2026-09-16, `phi_regs::memory::RING_REGION_BYTES`; 1 MiB when v1 was first cut, see the history below),
at card physical address `PHI_RING_REGION_BASE`. v1 uses `0x10000000`
(256 MiB), above the 64 MiB download address and the 128 MiB initramfs slot
that Intel's loader used. (v1 first placed it at 32 MiB, below the download
address; bulk writes there reset the host on 2026-09-14, see
`docs/results/2026-09-13-p3-kernel-build.md`. Nothing of Intel's ever wrote
below the download address.) The card kernel reserves it with
`memmap=1M$0x10000000` on its command line (written by the host loader) and
the `phinet` module `ioremap_cache`s it. The host reaches it at BAR0 offset
`PHI_RING_REGION_BASE`.

## Layout

All fields little-endian, all structures 64-byte aligned so that no two
fields written by different sides share a cache line.

```
struct phi_region_hdr {          // at offset 0
    u32 magic;                   // 'P','H','I','R' = 0x52494850
    u32 version;                 // 1
    u32 region_size;             // bytes, whole region
    u32 channel_count;           // number of phi_channel_desc that follow
    u64 host_epoch_ns;           // host wall clock at boot, for the card's clock
    u64 card_boot_flags;         // written by card: bit 0 = kernel reached init
    u8  pad[64 - 32];
};

struct phi_channel_desc {        // 64 bytes each, starting at offset 64
    u32 kind;                    // 1 = console, 2 = network
    u32 flags;
    u32 h2c_offset;              // offset of host-to-card ring (phi_ring)
    u32 h2c_size;                // data bytes in that ring (power of two)
    u32 c2h_offset;
    u32 c2h_size;
    u8  pad[64 - 24];
};

struct phi_ring {                // one per direction
    u32 magic;                   // 'R','I','N','G'
    u32 size;                    // data bytes, power of two
    u8  pad0[56];
    u32 head;                    // producer write index (monotonic, mod 2^32)
    u8  pad1[60];
    u32 tail;                    // consumer read index
    u8  pad2[60];
    u8  data[size];
};
```

Producer writes `data[head % size]`, then publishes `head`. Consumer reads up
to `head - tail` bytes from `tail`, then publishes `tail`. Indices are
free-running; `head - tail <= size` always. Wraparound is handled by the
modulo, so a message may span the end of `data`.

Console channel carries raw bytes. Network channel carries frames as
`u16 length` followed by `length` bytes of Ethernet frame, padded to 4 bytes.

## Signaling

v1: both sides poll. Host polls at 1 kHz for console, on demand for network.
Card polls from a kernel thread with `schedule_timeout`.

v2 (phase P6): host to card, write `APICICRn` with a vector registered by
`phinet`; card to host, write `RDMASR0`, delivered as MSI-X vector 0 to the
host's eventfd.

## Ordering

- Card side: `smp_wmb()` equivalent before publishing `head`/`tail`. Since
  KNC lacks `sfence`, the kernel's barrier definitions under the KNC config
  use `lock addl $0, (%rsp)`.
- Host side: data is written through an uncached BAR mapping. Before
  publishing an index the host performs a read-back of the ring `magic` to
  drain posted writes.

## Bring-up use

The kernel's early console (`card/kernel/platform/knc_earlycon.c`) writes
into the console `c2h` ring directly, before `phinet` loads, using the same
layout. The host's `phictl console` command tails it. This is how the first
kernel message reaches the host.

## Rpc channel (kind 3)

Added 2026-09-14 for the host tool that drives the card without SSH
(`docs/decisions/0008-direct-access-tool.md`). Two byte rings like the
others; the payload is a stream of frames defined by `host/crates/phi-rpc`:

```
frame  := u32 length (little-endian, of tag plus body) | u8 tag | body
string := u16 length | UTF-8 bytes
```

Tags: 1 Exec (u16 argc, strings; u16 envc, key/value strings; cwd string,
empty for none), 2 Stdin (bytes), 3 StdinEof, 4 Stdout (bytes), 5 Stderr
(bytes), 6 Exit (u32 status), 7 PutOpen (path string, u32 mode), 8 PutData
(bytes), 9 PutClose, 10 Get (path string), 11 GetData (bytes), 12 GetEnd
(u64 size), 13 Error (string), 14 Ping, 15 Pong (version string). Frames are
at most 1 MiB. One session at a time: the host sends a request (Exec,
PutOpen, Get or Ping) and the card answers with frames ending in Exit,
GetEnd, Pong or Error.

On the card the channel is `/dev/phirpc` (kernel patch 0022); on the host
`phictl boot --serve` reads and writes the rings through `phi-ring`. The
default plan gives the channel 256 KiB per direction, which is why the
region grew from 1 MiB to 2 MiB (`phictl --ring-size` default `0x200000`).

## Block channel (kind 4)

The card's `/dev/phiblk0` (kernel patch 0025, `arch/x86/kernel/knc_blk.c`)
and the host's `phictl boot --disk PATH` (`host/crates/phictl/src/disk.rs`)
carry fixed-size records instead of a byte stream, and the channel has a
data area after its rings (descriptor fields `DATA_OFFSET` at 24 and
`DATA_SIZE` at 28, 0 for channels without one) holding bounce slots of
512 KiB. Card to host, one 32-byte record per block request,
little-endian:

| offset | field | meaning |
| --- | --- | --- |
| 0 | tag u32 | the request's tag, which is also its slot number; `0xffffffff` for identify |
| 4 | op u32 | 0 read, 1 write, 2 flush, 3 identify |
| 8 | len u32 | bytes, a multiple of 512, at most 524288; 0 for flush and identify |
| 12 | reserved u32 | 0 |
| 16 | sector u64 | first 512-byte sector |
| 24 | phys u64 | card physical address of the request's slot |

Host to card, 8 bytes per completion: tag u32, status u32 (0 ok, else an
errno; the capacity in sectors for identify, 0 meaning no disk).

Two data paths, chosen by the host through the tag of its identify
answer:

- `0xfffffffe`, direct (the default): the card posts one record per
  physical segment of a request, tag = request tag `<< 8` plus the segment
  index, `phys` = the segment's address, and no data is copied on the card.
- `0xffffffff`, bounce: the card uses one record per request with `phys`
  = its 512 KiB slot in the data area (uncached, like the rings), copying
  its pages in before posting a write and out after a read completes. Kept
  for experiments (`PHICTL_DISK_BOUNCE=1` on the host): both host paths,
  aperture and DMA engine, proved coherent with the card's caches on
  2026-09-16, and the corruption that motivated the bounce came from torn
  ring indices and from the DMA engine's tail pointer running ahead of its
  writes (`docs/results/2026-09-16-dma.md`).

`knc_blk.direct=0` or `1` on the card's command line overrides the choice.

Sizes: 16 KiB host-to-card, 64 KiB card-to-host, 8 MiB data area (16
slots, the queue depth); the default region grew from 2 to 16 MiB for it.
Both ends poll: a spin of a few milliseconds after activity, then 200 us
sleeps. The identify record is posted by the driver's initcall and
answered once the host tool serves (after POST K7); `init` waits up to 5 s
for the device. Indices are single 32-bit accesses on both sides: the
host's byte-wise index writes were the cause of torn heads before
2026-09-16.

## Host memory (header fields and channel kind 5)

`phictl boot --host-mem SIZE` pins a shared file (`/dev/shm/phi-hostmem`,
mode 0600) for the card at IOMMU address 4 GiB and announces it in two
region header fields: `HOSTMEM_ADDR` (u64 at 32, the card address
`0x80_0000_0000 + IOMMU address`, reachable through the SMPT whose first
pages the host maps identity) and `HOSTMEM_SIZE` (u64 at 40, 0 when there
is none). Two consumers on the card (kernel patch 0026):

- Channel kind 5 carries the block records of kind 4 for a second device,
  `/dev/phiblk1`, backed by that memory. The host serves it with the DMA
  engine straight between the window and the card's pages (no staging),
  and `init` runs `mkswap` and `swapon` on it at every boot: host RAM as
  the card's swap, sized by `--host-mem`. Without the engine the channel
  has no bounce slots and no device appears.
- `/dev/phihost` (`knc_hostmem.c`) maps the same bytes uncached: `read`,
  `write`, `llseek` (the size is the end) and `mmap`. A card program and
  a host program that maps `/dev/shm/phi-hostmem` share memory with no
  copy; every card access is a PCIe transaction, so this is an exchange
  surface, and bulk traffic belongs on `/dev/phiblk1`.
