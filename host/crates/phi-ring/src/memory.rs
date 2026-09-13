//! The memory abstraction the protocol runs on.

/// Byte-addressable memory holding a ring region.
///
/// Implementations: [`VecMemory`] for tests, and the aperture mapping in
/// `phi-hw` for the card. All offsets are relative to the region base.
pub trait RingMemory {
    /// Copy bytes out.
    fn read(&self, off: usize, buf: &mut [u8]);
    /// Copy bytes in.
    fn write(&mut self, off: usize, data: &[u8]);
    /// Make prior writes visible to the other side before a later index
    /// publish. On the card backend this is a read-back of the ring magic to
    /// drain posted PCIe writes; on plain memory it is a compiler fence.
    fn fence(&mut self);

    /// Little-endian u32 read.
    fn read_u32(&self, off: usize) -> u32 {
        let mut b = [0u8; 4];
        self.read(off, &mut b);
        u32::from_le_bytes(b)
    }
    /// Little-endian u32 write.
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
    /// Length.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    /// True if empty.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl RingMemory for VecMemory {
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

impl<M: RingMemory + ?Sized> RingMemory for &mut M {
    fn read(&self, off: usize, buf: &mut [u8]) {
        (**self).read(off, buf)
    }
    fn write(&mut self, off: usize, data: &[u8]) {
        (**self).write(off, data)
    }
    fn fence(&mut self) {
        (**self).fence()
    }
}
