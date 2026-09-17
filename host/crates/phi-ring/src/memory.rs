//! The memory abstraction the protocol runs on.

/// Byte-addressable memory holding a ring region.
///
/// Implementations: [`VecMemory`] for tests, and the aperture mapping in
/// `phi-hw` for the card. All offsets are relative to the region base.
///
/// Ring indices (`head`, `tail`) are read and written only through
/// `read_u32` and `write_u32` at 4-aligned offsets. A backend on device
/// memory must implement both as one 32-bit access: the defaults go through
/// `read`/`write`, and a byte-wise copy lets the other side observe a torn
/// index (the block device hang of 2026-09-16, `phi-hw/src/ringmem.md`).
pub trait RingMemory {
    /// Length of the region in bytes; every offset must stay below it.
    fn len(&self) -> usize;
    /// Copy bytes out.
    fn read(&self, off: usize, buf: &mut [u8]);
    /// Copy bytes in.
    fn write(&mut self, off: usize, data: &[u8]);
    /// Make prior writes visible to the other side before a later index
    /// publish. On the card backend this is a read from the region, which
    /// drains posted PCIe writes; on plain memory it is a compiler fence.
    fn fence(&mut self);

    /// True when the region has no bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Little-endian u32 read (one 32-bit access on device memory).
    fn read_u32(&self, off: usize) -> u32 {
        let mut b = [0u8; 4];
        self.read(off, &mut b);
        u32::from_le_bytes(b)
    }
    /// Little-endian u32 write (one 32-bit access on device memory).
    fn write_u32(&mut self, off: usize, v: u32) {
        self.write(off, &v.to_le_bytes());
    }
    /// Little-endian u64 read.
    fn read_u64(&self, off: usize) -> u64 {
        let mut b = [0u8; 8];
        self.read(off, &mut b);
        u64::from_le_bytes(b)
    }
    /// Little-endian u64 write.
    fn write_u64(&mut self, off: usize, v: u64) {
        self.write(off, &v.to_le_bytes());
    }
}

/// Plain memory backend for tests and for formatting an image offline.
#[derive(Clone, Debug)]
pub struct VecMemory {
    bytes: Vec<u8>,
}

impl VecMemory {
    /// Zeroed memory of `len` bytes.
    pub fn new(len: usize) -> Self {
        Self { bytes: vec![0; len] }
    }
    /// Borrow the bytes (for writing a pre-formatted region to the card in one go).
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl RingMemory for VecMemory {
    fn len(&self) -> usize {
        self.bytes.len()
    }
    fn read(&self, off: usize, buf: &mut [u8]) {
        buf.copy_from_slice(&self.bytes[off..off + buf.len()]);
    }
    fn write(&mut self, off: usize, data: &[u8]) {
        self.bytes[off..off + data.len()].copy_from_slice(data);
    }
    fn fence(&mut self) {
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

// Every method is forwarded, the u32/u64 accessors included: a `&mut
// ApertureRegion` used as the backend must keep its single 32-bit index
// accesses rather than fall back to the byte-wise defaults.
impl<M: RingMemory + ?Sized> RingMemory for &mut M {
    fn len(&self) -> usize {
        (**self).len()
    }
    fn read(&self, off: usize, buf: &mut [u8]) {
        (**self).read(off, buf)
    }
    fn write(&mut self, off: usize, data: &[u8]) {
        (**self).write(off, data)
    }
    fn fence(&mut self) {
        (**self).fence()
    }
    fn is_empty(&self) -> bool {
        (**self).is_empty()
    }
    fn read_u32(&self, off: usize) -> u32 {
        (**self).read_u32(off)
    }
    fn write_u32(&mut self, off: usize, v: u32) {
        (**self).write_u32(off, v)
    }
    fn read_u64(&self, off: usize) -> u64 {
        (**self).read_u64(off)
    }
    fn write_u64(&mut self, off: usize, v: u64) {
        (**self).write_u64(off, v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A backend that counts how its index accessors are reached, standing
    /// in for the aperture backend whose `read_u32`/`write_u32` are single
    /// 32-bit accesses.
    struct Counting {
        inner: VecMemory,
        u32_reads: Cell<u32>,
        u32_writes: u32,
        byte_reads: Cell<u32>,
    }

    impl RingMemory for Counting {
        fn len(&self) -> usize {
            self.inner.len()
        }
        fn read(&self, off: usize, buf: &mut [u8]) {
            self.byte_reads.set(self.byte_reads.get() + 1);
            self.inner.read(off, buf)
        }
        fn write(&mut self, off: usize, data: &[u8]) {
            self.inner.write(off, data)
        }
        fn fence(&mut self) {}
        fn read_u32(&self, off: usize) -> u32 {
            self.u32_reads.set(self.u32_reads.get() + 1);
            self.inner.read_u32(off)
        }
        fn write_u32(&mut self, off: usize, v: u32) {
            self.u32_writes += 1;
            self.inner.write_u32(off, v)
        }
    }

    /// Reaches the backend the way the ring code does: through a generic
    /// parameter, here instantiated with the reference type itself.
    fn through_generic<M: RingMemory>(mem: &mut M) {
        mem.write_u32(8, 0xA5A5_0001);
        assert_eq!(mem.read_u32(8), 0xA5A5_0001);
        assert_eq!(mem.len(), 64);
        assert!(!mem.is_empty());
    }

    #[test]
    fn reference_backend_keeps_the_single_access_index_paths() {
        let mut backend = Counting {
            inner: VecMemory::new(64),
            u32_reads: Cell::new(0),
            u32_writes: 0,
            byte_reads: Cell::new(0),
        };
        through_generic(&mut &mut backend);
        assert_eq!(backend.u32_writes, 1);
        assert_eq!(backend.u32_reads.get(), 1);
        assert_eq!(backend.byte_reads.get(), 0, "the u32 read must not go through the byte path");
    }

    #[test]
    fn little_endian_accessors_round_trip() {
        let mut mem = VecMemory::new(32);
        mem.write_u64(16, 0x0102_0304_0506_0708);
        assert_eq!(mem.read_u64(16), 0x0102_0304_0506_0708);
        assert_eq!(mem.read_u32(16), 0x0506_0708, "low dword first");
        assert_eq!(mem.as_bytes()[16], 0x08);
        assert!(VecMemory::new(0).is_empty());
    }
}
