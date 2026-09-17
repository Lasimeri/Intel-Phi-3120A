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

fn ring_data_size_ok(size: u32) -> Result<(), Error> {
    if size == 0 || !size.is_power_of_two() {
        return Err(Error::BadRingSize(size));
    }
    Ok(())
}

/// Compute where the rings and data areas of `plans` go: rings in channel
/// order after the descriptor table, each `h2c` before `c2h`, each on a
/// 64-byte boundary; every data area after the last ring, page aligned.
/// Returns the views and the first byte past the layout.
fn lay_out(plans: &[ChannelPlan]) -> Result<(Vec<ChannelView>, usize), Error> {
    for p in plans {
        ring_data_size_ok(p.h2c_size)?;
        ring_data_size_ok(p.c2h_size)?;
    }
    let table_end = REGION_HDR_SIZE + plans.len() * CHANNEL_DESC_SIZE;
    let mut cursor = align_up(table_end, LINE);
    let mut views = Vec::with_capacity(plans.len());
    for p in plans {
        let h2c = cursor;
        cursor = align_up(cursor + RING_HDR_SIZE + p.h2c_size as usize, LINE);
        let c2h = cursor;
        cursor = align_up(cursor + RING_HDR_SIZE + p.c2h_size as usize, LINE);
        views.push(ChannelView {
            kind: p.kind,
            h2c_offset: h2c,
            c2h_offset: c2h,
            data_offset: 0,
            data_size: p.data_size,
        });
    }
    for v in views.iter_mut() {
        if v.data_size > 0 {
            cursor = align_up(cursor, DATA_ALIGN);
            v.data_offset = cursor;
            cursor = align_up(cursor + v.data_size as usize, DATA_ALIGN);
        }
    }
    Ok((views, cursor))
}

impl Region {
    /// Format `mem` (of `region_size` bytes) with the given channels.
    /// Rings are laid out after the channel table, each 64-byte aligned.
    /// `host_epoch_ns` is the host wall clock the card will adopt;
    /// `hostmem` the (card address, size) of the host memory window, if
    /// any. Everything is validated before the first write, and the
    /// region magic is cleared first and written last, so a reader that
    /// sees the magic sees a complete table.
    pub fn format<M: RingMemory>(
        mem: &mut M,
        region_size: usize,
        plans: &[ChannelPlan],
        host_epoch_ns: u64,
        hostmem: Option<(u64, u64)>,
    ) -> Result<Self, Error> {
        if region_size > mem.len() {
            return Err(Error::DoesNotFit(region_size, mem.len()));
        }
        if u32::try_from(region_size).is_err() || u32::try_from(plans.len()).is_err() {
            return Err(Error::BadLayout(format!(
                "region of {region_size:#x} bytes with {} channels does not fit the 32-bit header fields",
                plans.len()
            )));
        }
        let (views, end) = lay_out(plans)?;
        if end > region_size {
            return Err(Error::DoesNotFit(end, region_size));
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

    /// The layout the formatter would produce for `plans`: the channel
    /// views and the first byte past the last ring or data area. For
    /// checking a plan against a region size without formatting anything.
    pub fn plan_layout(plans: &[ChannelPlan]) -> Result<(Vec<ChannelView>, usize), Error> {
        lay_out(plans)
    }

    /// Open an already formatted region and read its channel table.
    ///
    /// Every offset in the header is checked against the recorded region
    /// size before it is used, and the recorded size against `mem`, so a
    /// header that was formatted for a different size, or overwritten,
    /// is reported as an error instead of an out-of-bounds access in the
    /// backend. Descriptors of unknown kinds are skipped (a newer host may
    /// have formatted them) and not validated.
    pub fn open<M: RingMemory>(mem: &M) -> Result<Self, Error> {
        if mem.len() < REGION_HDR_SIZE {
            return Err(Error::BadLayout(format!(
                "memory of {:#x} bytes is smaller than the header",
                mem.len()
            )));
        }
        let magic = mem.read_u32(region_hdr::MAGIC);
        if magic != REGION_MAGIC {
            return Err(Error::BadMagic(magic, REGION_MAGIC));
        }
        let version = mem.read_u32(region_hdr::VERSION);
        if version != VERSION {
            return Err(Error::BadVersion(version, VERSION));
        }
        let size = mem.read_u32(region_hdr::REGION_SIZE) as usize;
        if size > mem.len() {
            return Err(Error::BadLayout(format!(
                "header says {size:#x} bytes but the memory is {:#x} (formatted with another --ring-size?)",
                mem.len()
            )));
        }
        let count = mem.read_u32(region_hdr::CHANNEL_COUNT) as usize;
        let table_end = count.checked_mul(CHANNEL_DESC_SIZE).and_then(|t| t.checked_add(REGION_HDR_SIZE));
        if table_end.is_none_or(|end| end > size) {
            return Err(Error::BadLayout(format!(
                "{count} channel descriptors do not fit in {size:#x} bytes"
            )));
        }
        let mut channels = Vec::with_capacity(count);
        for i in 0..count {
            let d = REGION_HDR_SIZE + i * CHANNEL_DESC_SIZE;
            let Some(kind) = ChannelKind::from_u32(mem.read_u32(d + channel_desc::KIND)) else {
                continue;
            };
            let h2c_offset = mem.read_u32(d + channel_desc::H2C_OFFSET) as usize;
            let c2h_offset = mem.read_u32(d + channel_desc::C2H_OFFSET) as usize;
            let h2c_size = mem.read_u32(d + channel_desc::H2C_SIZE);
            let c2h_size = mem.read_u32(d + channel_desc::C2H_SIZE);
            for (name, off, ring_size) in [("h2c", h2c_offset, h2c_size), ("c2h", c2h_offset, c2h_size)] {
                ring_data_size_ok(ring_size)?;
                let end = off.checked_add(RING_HDR_SIZE + ring_size as usize);
                if !off.is_multiple_of(LINE) || end.is_none_or(|e| e > size) {
                    return Err(Error::BadLayout(format!(
                        "{kind:?} {name} ring at {off:#x} with {ring_size:#x} data bytes is outside the {size:#x}-byte region or misaligned"
                    )));
                }
            }
            let data_offset = mem.read_u32(d + channel_desc::DATA_OFFSET) as usize;
            let data_size = mem.read_u32(d + channel_desc::DATA_SIZE);
            if data_size > 0 || data_offset > 0 {
                let end = data_offset.checked_add(data_size as usize);
                if !data_offset.is_multiple_of(DATA_ALIGN) || data_size == 0 || end.is_none_or(|e| e > size) {
                    return Err(Error::BadLayout(format!(
                        "{kind:?} data area at {data_offset:#x} with {data_size:#x} bytes is outside the {size:#x}-byte region or misaligned"
                    )));
                }
            }
            channels.push(ChannelView {
                kind,
                h2c_offset,
                c2h_offset,
                data_offset,
                data_size,
            });
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
        assert_eq!(Region::hostmem_window(&mem), None);
        assert_eq!(Region::card_boot_flags(&mem), 0);
        // Host writes console h2c; a simulated card reads it.
        let (p, _c) = o.host_endpoints(&mem, ChannelKind::Console).unwrap();
        assert_eq!(p.push(&mut mem, b"hello card"), 10);
        let card_consumer = Consumer::attach(&mem, con.h2c_offset).unwrap();
        let mut buf = [0u8; 10];
        assert_eq!(card_consumer.pop(&mut mem, &mut buf), 10);
        assert_eq!(&buf, b"hello card");
    }

    #[test]
    fn data_areas_and_hostmem_fields_round_trip() {
        let plans = [
            ChannelPlan {
                kind: ChannelKind::Rpc,
                h2c_size: 1024,
                c2h_size: 1024,
                data_size: 0,
            },
            ChannelPlan {
                kind: ChannelKind::Block,
                h2c_size: 16384,
                c2h_size: 65536,
                data_size: 3 * 4096 + 100,
            },
            ChannelPlan {
                kind: ChannelKind::HostMem,
                h2c_size: 512,
                c2h_size: 512,
                data_size: 0,
            },
        ];
        let mut mem = VecMemory::new(256 * 1024);
        let len = mem.len();
        let r = Region::format(&mut mem, len, &plans, 7, Some((0x80_0000_0000 + (1 << 32), 1 << 30))).unwrap();
        let o = Region::open(&mem).unwrap();
        assert_eq!(o.channels(), r.channels());
        let blk = o.channel(ChannelKind::Block).unwrap();
        assert_eq!(blk.data_offset % DATA_ALIGN, 0);
        assert_eq!(blk.data_size, 3 * 4096 + 100);
        // The data area comes after every ring, including the later channel's.
        let hm = o.channel(ChannelKind::HostMem).unwrap();
        assert!(blk.data_offset > hm.c2h_offset + RING_HDR_SIZE + 512);
        assert_eq!(hm.data_offset, 0);
        assert_eq!(Region::hostmem_window(&mem), Some((0x81_0000_0000, 1 << 30)));
        let (views, end) = Region::plan_layout(&plans).unwrap();
        assert_eq!(views, r.channels());
        assert_eq!(end, blk.data_offset + align_up(blk.data_size as usize, DATA_ALIGN));
        assert!(o.channel(ChannelKind::Console).is_err());
    }

    #[test]
    fn refuses_layouts_that_do_not_fit() {
        let mut mem = VecMemory::new(4096);
        let len = mem.len();
        let e = Region::format(&mut mem, len, &PLANS, 0, None).unwrap_err();
        assert!(matches!(e, Error::DoesNotFit(_, 4096)));
        // Nothing was written because validation happens first.
        assert_eq!(mem.read_u32(region_hdr::MAGIC), 0);
        // A region size larger than the backing memory is refused too.
        let e = Region::format(&mut mem, 8192, &PLANS, 0, None).unwrap_err();
        assert_eq!(e, Error::DoesNotFit(8192, 4096));
        let bad = [ChannelPlan {
            kind: ChannelKind::Console,
            h2c_size: 100,
            c2h_size: 128,
            data_size: 0,
        }];
        assert_eq!(Region::format(&mut mem, len, &bad, 0, None).unwrap_err(), Error::BadRingSize(100));
    }

    #[test]
    fn unformatted_memory_is_rejected() {
        let mem = VecMemory::new(4096);
        assert_eq!(Region::open(&mem).err(), Some(Error::BadMagic(0, REGION_MAGIC)));
        assert!(matches!(Region::open(&VecMemory::new(32)).unwrap_err(), Error::BadLayout(_)));
        let mut mem = VecMemory::new(4096);
        mem.write_u32(region_hdr::MAGIC, REGION_MAGIC);
        mem.write_u32(region_hdr::VERSION, 2);
        assert_eq!(Region::open(&mem).err(), Some(Error::BadVersion(2, VERSION)));
    }

    #[test]
    fn open_rejects_headers_that_point_outside_the_memory() {
        let mut mem = VecMemory::new(1024 * 1024);
        let len = mem.len();
        Region::format(&mut mem, len, &PLANS, 0, None).unwrap();
        // The same bytes seen through a smaller window (another --ring-size).
        let mut small = VecMemory::new(4096);
        small.write(0, &mem.as_bytes()[..4096]);
        assert!(matches!(Region::open(&small).unwrap_err(), Error::BadLayout(_)));

        // A channel count that would walk past the region.
        let mut m = mem.clone();
        m.write_u32(region_hdr::CHANNEL_COUNT, u32::MAX);
        assert!(matches!(Region::open(&m).unwrap_err(), Error::BadLayout(_)));

        // A ring offset past the region end, and a misaligned one.
        let d = REGION_HDR_SIZE + CHANNEL_DESC_SIZE;
        let mut m = mem.clone();
        m.write_u32(d + channel_desc::C2H_OFFSET, (len - 100) as u32);
        assert!(matches!(Region::open(&m).unwrap_err(), Error::BadLayout(_)));
        let mut m = mem.clone();
        m.write_u32(d + channel_desc::H2C_OFFSET, 4100);
        assert!(matches!(Region::open(&m).unwrap_err(), Error::BadLayout(_)));
        let mut m = mem.clone();
        m.write_u32(d + channel_desc::H2C_SIZE, 3000);
        assert_eq!(Region::open(&m).unwrap_err(), Error::BadRingSize(3000));

        // A data area outside the region.
        let mut m = mem.clone();
        m.write_u32(d + channel_desc::DATA_OFFSET, (len - 4096) as u32);
        m.write_u32(d + channel_desc::DATA_SIZE, 8192);
        assert!(matches!(Region::open(&m).unwrap_err(), Error::BadLayout(_)));

        // An unknown kind is skipped, whatever its offsets say.
        let mut m = mem.clone();
        m.write_u32(d + channel_desc::KIND, 99);
        m.write_u32(d + channel_desc::C2H_OFFSET, u32::MAX);
        let o = Region::open(&m).unwrap();
        assert_eq!(o.channels().len(), 1);
        assert_eq!(o.channel(ChannelKind::Network).unwrap_err(), Error::NoChannel(ChannelKind::Network));
    }
}
