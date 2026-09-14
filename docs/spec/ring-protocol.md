# Ring protocol v1: host/card shared-memory transport

Status: specified; host implementation in `host/crates/phi-ring`; card
implementation planned in `card/drivers/phinet`.

## Placement

A contiguous region of card GDDR, `PHI_RING_REGION_SIZE` bytes (1 MiB in v1),
at card physical address `PHI_RING_REGION_BASE`. v1 uses `0x10000000`
(256 MiB), above the 64 MiB download address and the 128 MiB initramfs slot
that Intel's loader used. (v1 first placed it at 32 MiB, below the download
address; bulk writes there reset the host on 2026-09-14, see
`docs/results/2026-09-13-p3-kernel-build.md`. Nothing of Intel's ever wrote
below the download address.) The card kernel reserves it with
`memmap=1M$0x2000000` on its command line (written by the host loader) and
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
