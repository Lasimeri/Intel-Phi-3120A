//! Single-producer / single-consumer byte ring over [`RingMemory`].
//!
//! Indices are free-running `u32` values; the data offset is
//! `index & (size - 1)`. Because `size` is a power of two and `head - tail`
//! (wrapping) never exceeds `size`, the arithmetic is exact across the
//! `u32` wraparound.

use crate::layout::{ring_hdr, RING_MAGIC};
use crate::memory::RingMemory;
use crate::Error;

/// Validate a ring header at `base` and return its data size.
pub fn ring_size<M: RingMemory>(mem: &M, base: usize) -> Result<u32, Error> {
    let magic = mem.read_u32(base + ring_hdr::MAGIC);
    if magic != RING_MAGIC {
        return Err(Error::BadMagic(magic, RING_MAGIC));
    }
    let size = mem.read_u32(base + ring_hdr::SIZE);
    if size == 0 || !size.is_power_of_two() {
        return Err(Error::BadRingSize(size));
    }
    Ok(size)
}

/// Write a fresh ring header (magic, size, head = tail = 0) at `base`.
pub fn format_ring<M: RingMemory>(mem: &mut M, base: usize, size: u32) -> Result<(), Error> {
    if size == 0 || !size.is_power_of_two() {
        return Err(Error::BadRingSize(size));
    }
    mem.write_u32(base + ring_hdr::HEAD, 0);
    mem.write_u32(base + ring_hdr::TAIL, 0);
    mem.write_u32(base + ring_hdr::SIZE, size);
    mem.fence();
    mem.write_u32(base + ring_hdr::MAGIC, RING_MAGIC);
    mem.fence();
    Ok(())
}

/// The producing end of a ring.
pub struct Producer {
    base: usize,
    size: u32,
}

impl Producer {
    /// Attach to a ring at `base` (validates the header).
    pub fn attach<M: RingMemory>(mem: &M, base: usize) -> Result<Self, Error> {
        Ok(Self {
            base,
            size: ring_size(mem, base)?,
        })
    }

    /// Data capacity in bytes.
    pub fn capacity(&self) -> u32 {
        self.size
    }

    /// Bytes that can be pushed right now.
    pub fn free<M: RingMemory>(&self, mem: &M) -> u32 {
        let head = mem.read_u32(self.base + ring_hdr::HEAD);
        let tail = mem.read_u32(self.base + ring_hdr::TAIL);
        self.size - head.wrapping_sub(tail)
    }

    /// Push as much of `data` as fits; returns the number of bytes pushed.
    pub fn push<M: RingMemory>(&self, mem: &mut M, data: &[u8]) -> usize {
        let head = mem.read_u32(self.base + ring_hdr::HEAD);
        let tail = mem.read_u32(self.base + ring_hdr::TAIL);
        let free = self.size - head.wrapping_sub(tail);
        let n = data.len().min(free as usize);
        if n == 0 {
            return 0;
        }
        let mask = (self.size - 1) as usize;
        let start = (head as usize) & mask;
        let first = n.min(self.size as usize - start);
        mem.write(self.base + ring_hdr::DATA + start, &data[..first]);
        if first < n {
            mem.write(self.base + ring_hdr::DATA, &data[first..n]);
        }
        mem.fence();
        mem.write_u32(self.base + ring_hdr::HEAD, head.wrapping_add(n as u32));
        mem.fence();
        n
    }
}

/// The consuming end of a ring.
pub struct Consumer {
    base: usize,
    size: u32,
}

impl Consumer {
    /// Attach to a ring at `base` (validates the header).
    pub fn attach<M: RingMemory>(mem: &M, base: usize) -> Result<Self, Error> {
        Ok(Self {
            base,
            size: ring_size(mem, base)?,
        })
    }

    /// Bytes available to pop.
    pub fn available<M: RingMemory>(&self, mem: &M) -> u32 {
        let head = mem.read_u32(self.base + ring_hdr::HEAD);
        let tail = mem.read_u32(self.base + ring_hdr::TAIL);
        head.wrapping_sub(tail)
    }

    /// Pop up to `buf.len()` bytes; returns the number popped.
    pub fn pop<M: RingMemory>(&self, mem: &mut M, buf: &mut [u8]) -> usize {
        let head = mem.read_u32(self.base + ring_hdr::HEAD);
        let tail = mem.read_u32(self.base + ring_hdr::TAIL);
        let avail = head.wrapping_sub(tail);
        let n = buf.len().min(avail as usize);
        if n == 0 {
            return 0;
        }
        let mask = (self.size - 1) as usize;
        let start = (tail as usize) & mask;
        let first = n.min(self.size as usize - start);
        mem.read(self.base + ring_hdr::DATA + start, &mut buf[..first]);
        if first < n {
            mem.read(self.base + ring_hdr::DATA, &mut buf[first..n]);
        }
        mem.fence();
        mem.write_u32(self.base + ring_hdr::TAIL, tail.wrapping_add(n as u32));
        mem.fence();
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::VecMemory;

    fn ring(size: u32) -> (VecMemory, Producer, Consumer) {
        let mut mem = VecMemory::new(ring_hdr::DATA + size as usize);
        format_ring(&mut mem, 0, size).unwrap();
        let p = Producer::attach(&mem, 0).unwrap();
        let c = Consumer::attach(&mem, 0).unwrap();
        (mem, p, c)
    }

    #[test]
    fn round_trip_with_wraparound() {
        let (mut mem, p, c) = ring(16);
        assert_eq!(p.free(&mem), 16);
        assert_eq!(p.push(&mut mem, b"0123456789ab"), 12);
        let mut out = [0u8; 8];
        assert_eq!(c.pop(&mut mem, &mut out), 8);
        assert_eq!(&out, b"01234567");
        // 4 bytes remain; push 10 more: 12 bytes total, spanning the end.
        assert_eq!(p.push(&mut mem, b"CDEFGHIJKL"), 10);
        assert_eq!(c.available(&mem), 14);
        let mut out = [0u8; 14];
        assert_eq!(c.pop(&mut mem, &mut out), 14);
        assert_eq!(&out, b"89abCDEFGHIJKL");
        assert_eq!(c.available(&mem), 0);
    }

    #[test]
    fn full_ring_refuses_and_partial_push_reports_count() {
        let (mut mem, p, c) = ring(8);
        assert_eq!(p.push(&mut mem, b"abcdefghij"), 8, "only capacity bytes accepted");
        assert_eq!(p.push(&mut mem, b"x"), 0);
        let mut out = [0u8; 3];
        assert_eq!(c.pop(&mut mem, &mut out), 3);
        assert_eq!(p.free(&mem), 3);
    }

    #[test]
    fn indices_survive_u32_wraparound() {
        let (mut mem, p, c) = ring(8);
        // Force head and tail near u32::MAX.
        mem.write_u32(ring_hdr::HEAD, u32::MAX - 2);
        mem.write_u32(ring_hdr::TAIL, u32::MAX - 2);
        assert_eq!(p.push(&mut mem, b"12345"), 5);
        let mut out = [0u8; 5];
        assert_eq!(c.pop(&mut mem, &mut out), 5);
        assert_eq!(&out, b"12345");
        assert_eq!(mem.read_u32(ring_hdr::HEAD), 2, "wrapped");
    }

    #[test]
    fn rejects_bad_headers() {
        let mut mem = VecMemory::new(ring_hdr::DATA + 16);
        assert_eq!(Producer::attach(&mem, 0).err(), Some(Error::BadMagic(0, RING_MAGIC)));
        assert_eq!(format_ring(&mut mem, 0, 12).err(), Some(Error::BadRingSize(12)));
    }
}
