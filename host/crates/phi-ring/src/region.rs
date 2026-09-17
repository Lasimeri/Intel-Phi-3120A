//! A whole region: header, channel table, and the rings they point at.

use crate::layout::*;
use crate::memory::RingMemory;
use crate::ring::{format_ring, Consumer, Producer};
use crate::Error;

/// What the formatter should create for one channel.
#[derive(Clone, Copy, Debug)]
pub struct ChannelPlan {
    /// Channel kind.
    pub kind: ChannelKind,
    /// Host-to-card data bytes (power of two).
    pub h2c_size: u32,
    /// Card-to-host data bytes (power of two).
    pub c2h_size: u32,
    /// Bytes of data area after the rings (0 for none). The block channel
    /// uses it for bounce slots.
    pub data_size: u32,
}

/// A channel as found in a formatted region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelView {
    /// Channel kind.
    pub kind: ChannelKind,
    /// Region offset of the host-to-card ring header.
    pub h2c_offset: usize,
    /// Region offset of the card-to-host ring header.
    pub c2h_offset: usize,
    /// Region offset of the data area (0 = none).
    pub data_offset: usize,
    /// Size of the data area.
    pub data_size: u32,
}

/// A validated region.
#[derive(Debug)]
pub struct Region {
    size: usize,
    channels: Vec<ChannelView>,
}

fn align_up(x: usize, a: usize) -> usize {
    x.div_ceil(a) * a
}

impl Region {
    /// Format `mem` (of `region_size` bytes) with the given channels.
    /// Rings are laid out after the channel table, each 64-byte aligned.
    /// `host_epoch_ns` is the host wall clock the card will adopt.
    pub fn format<M: RingMemory>(
        mem: &mut M,
        region_size: usize,
        plans: &[ChannelPlan],
        host_epoch_ns: u64,
        hostmem: Option<(u64, u64)>,
    ) -> Result<Self, Error> {
        for p in plans {
            for s in [p.h2c_size, p.c2h_size] {
                if s == 0 || !s.is_power_of_two() {
                    return Err(Error::BadRingSize(s));
                }
            }
        }
        let table_end = REGION_HDR_SIZE + plans.len() * CHANNEL_DESC_SIZE;
        let mut cursor = align_up(table_end, 64);
        let mut views = Vec::with_capacity(plans.len());
        for p in plans {
            let h2c = cursor;
            cursor = align_up(cursor + RING_HDR_SIZE + p.h2c_size as usize, 64);
            let c2h = cursor;
            cursor = align_up(cursor + RING_HDR_SIZE + p.c2h_size as usize, 64);
            views.push(ChannelView {
                kind: p.kind,
                h2c_offset: h2c,
                c2h_offset: c2h,
                data_offset: 0,
                data_size: p.data_size,
            });
        }
        // Data areas after every ring, page aligned.
        for v in views.iter_mut() {
            if v.data_size > 0 {
                cursor = align_up(cursor, 4096);
                v.data_offset = cursor;
                cursor = align_up(cursor + v.data_size as usize, 4096);
            }
        }
        if cursor > region_size {
            return Err(Error::DoesNotFit(cursor, region_size));
        }
        // Invalidate the old magic first so a reader never sees a stale
        // header pointing at rings being rewritten.
        mem.write_u32(region_hdr::MAGIC, 0);
        mem.fence();
        for (p, v) in plans.iter().zip(&views) {
            format_ring(mem, v.h2c_offset, p.h2c_size)?;
            format_ring(mem, v.c2h_offset, p.c2h_size)?;
        }
        for (i, (p, v)) in plans.iter().zip(&views).enumerate() {
            let d = REGION_HDR_SIZE + i * CHANNEL_DESC_SIZE;
            mem.write_u32(d + channel_desc::KIND, p.kind as u32);
            mem.write_u32(d + channel_desc::FLAGS, 0);
            mem.write_u32(d + channel_desc::H2C_OFFSET, v.h2c_offset as u32);
            mem.write_u32(d + channel_desc::H2C_SIZE, p.h2c_size);
            mem.write_u32(d + channel_desc::C2H_OFFSET, v.c2h_offset as u32);
            mem.write_u32(d + channel_desc::C2H_SIZE, p.c2h_size);
            mem.write_u32(d + channel_desc::DATA_OFFSET, v.data_offset as u32);
            mem.write_u32(d + channel_desc::DATA_SIZE, v.data_size);
        }
        mem.write_u32(region_hdr::VERSION, VERSION);
        mem.write_u32(region_hdr::REGION_SIZE, region_size as u32);
        mem.write_u32(region_hdr::CHANNEL_COUNT, plans.len() as u32);
        mem.write_u64(region_hdr::HOST_EPOCH_NS, host_epoch_ns);
        mem.write_u64(region_hdr::CARD_BOOT_FLAGS, 0);
        let (hm_addr, hm_size) = hostmem.unwrap_or((0, 0));
        mem.write_u64(region_hdr::HOSTMEM_ADDR, hm_addr);
        mem.write_u64(region_hdr::HOSTMEM_SIZE, hm_size);
        mem.fence();
        mem.write_u32(region_hdr::MAGIC, REGION_MAGIC);
        mem.fence();
        Ok(Self {
            size: region_size,
            channels: views,
        })
    }

    /// Open an already formatted region and read its channel table.
    pub fn open<M: RingMemory>(mem: &M) -> Result<Self, Error> {
        let magic = mem.read_u32(region_hdr::MAGIC);
        if magic != REGION_MAGIC {
            return Err(Error::BadMagic(magic, REGION_MAGIC));
        }
        let version = mem.read_u32(region_hdr::VERSION);
        if version != VERSION {
            return Err(Error::BadVersion(version, VERSION));
        }
        let size = mem.read_u32(region_hdr::REGION_SIZE) as usize;
        let count = mem.read_u32(region_hdr::CHANNEL_COUNT) as usize;
        let mut channels = Vec::with_capacity(count);
        for i in 0..count {
            let d = REGION_HDR_SIZE + i * CHANNEL_DESC_SIZE;
            let kind = ChannelKind::from_u32(mem.read_u32(d + channel_desc::KIND));
            let h2c_offset = mem.read_u32(d + channel_desc::H2C_OFFSET) as usize;
            let c2h_offset = mem.read_u32(d + channel_desc::C2H_OFFSET) as usize;
            if let Some(kind) = kind {
                channels.push(ChannelView {
                    kind,
                    h2c_offset,
                    c2h_offset,
                    data_offset: mem.read_u32(d + channel_desc::DATA_OFFSET) as usize,
                    data_size: mem.read_u32(d + channel_desc::DATA_SIZE),
                });
            }
        }
        Ok(Self { size, channels })
    }

    /// Region size recorded in the header.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Channels found.
    pub fn channels(&self) -> &[ChannelView] {
        &self.channels
    }

    /// Find a channel by kind.
    pub fn channel(&self, kind: ChannelKind) -> Result<ChannelView, Error> {
        self.channels.iter().copied().find(|c| c.kind == kind).ok_or(Error::NoChannel(kind))
    }

    /// Host endpoints for a channel: a producer on the host-to-card ring and
    /// a consumer on the card-to-host ring.
    pub fn host_endpoints<M: RingMemory>(&self, mem: &M, kind: ChannelKind) -> Result<(Producer, Consumer), Error> {
        let c = self.channel(kind)?;
        Ok((Producer::attach(mem, c.h2c_offset)?, Consumer::attach(mem, c.c2h_offset)?))
    }

    /// The host memory window announced in the header, if any.
    pub fn hostmem_window<M: RingMemory>(mem: &M) -> Option<(u64, u64)> {
        let size = mem.read_u64(region_hdr::HOSTMEM_SIZE);
        (size > 0).then(|| (mem.read_u64(region_hdr::HOSTMEM_ADDR), size))
    }

    /// Card-side flags word (`CARD_FLAG_*`).
    pub fn card_boot_flags<M: RingMemory>(mem: &M) -> u64 {
        mem.read_u64(region_hdr::CARD_BOOT_FLAGS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::VecMemory;

    const PLANS: [ChannelPlan; 2] = [
        ChannelPlan {
            kind: ChannelKind::Console,
            h2c_size: 4096,
            c2h_size: 65536,
            data_size: 0,
        },
        ChannelPlan {
            kind: ChannelKind::Network,
            h2c_size: 262_144,
            c2h_size: 262_144,
            data_size: 0,
        },
    ];

    #[test]
    fn format_then_open_round_trips() {
        let mut mem = VecMemory::new(1024 * 1024);
        let len = mem.len();
        let r = Region::format(&mut mem, len, &PLANS, 1_700_000_000_000_000_000, None).unwrap();
        let o = Region::open(&mem).unwrap();
        assert_eq!(o.channels(), r.channels());
        assert_eq!(o.size(), 1024 * 1024);
        let con = o.channel(ChannelKind::Console).unwrap();
        assert_eq!(con.h2c_offset % 64, 0);
        assert_eq!(con.c2h_offset % 64, 0);
        assert_eq!(mem.read_u64(region_hdr::HOST_EPOCH_NS), 1_700_000_000_000_000_000);
        // Host writes console h2c; a simulated card reads it.
        let (p, _c) = o.host_endpoints(&mem, ChannelKind::Console).unwrap();
        assert_eq!(p.push(&mut mem, b"hello card"), 10);
        let card_consumer = Consumer::attach(&mem, con.h2c_offset).unwrap();
        let mut buf = [0u8; 10];
        assert_eq!(card_consumer.pop(&mut mem, &mut buf), 10);
        assert_eq!(&buf, b"hello card");
    }

    #[test]
    fn refuses_layouts_that_do_not_fit() {
        let mut mem = VecMemory::new(4096);
        let len = mem.len();
        let e = Region::format(&mut mem, len, &PLANS, 0, None).unwrap_err();
        assert!(matches!(e, Error::DoesNotFit(_, 4096)));
        // Nothing was written because validation happens first.
        assert_eq!(mem.read_u32(region_hdr::MAGIC), 0);
    }

    #[test]
    fn unformatted_memory_is_rejected() {
        let mem = VecMemory::new(4096);
        assert_eq!(Region::open(&mem).err(), Some(Error::BadMagic(0, REGION_MAGIC)));
    }
}
