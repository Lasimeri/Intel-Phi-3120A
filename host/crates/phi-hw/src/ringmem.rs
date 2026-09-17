//! The aperture as a [`phi_ring::RingMemory`] backend.

use phi_ring::RingMemory;
use phi_vfio::mapping::Mapping;

/// A window of card memory at a fixed base, addressed region-relative.
pub struct ApertureRegion<'a> {
    aperture: &'a Mapping,
    base: usize,
    len: usize,
}

impl<'a> ApertureRegion<'a> {
    /// Region of `len` bytes at card physical `base`.
    pub fn new(aperture: &'a Mapping, base: usize, len: usize) -> Self {
        assert!(base + len <= aperture.len(), "ring region outside the aperture");
        Self { aperture, base, len }
    }

    /// Region length.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True if empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl RingMemory for ApertureRegion<'_> {
    fn len(&self) -> usize {
        self.len
    }
    fn read(&self, off: usize, buf: &mut [u8]) {
        assert!(off + buf.len() <= self.len, "ring read outside region");
        self.aperture.read_bytes(self.base + off, buf);
    }
    fn write(&mut self, off: usize, data: &[u8]) {
        assert!(off + data.len() <= self.len, "ring write outside region");
        self.aperture.write_bytes(self.base + off, data);
    }
    // Ring indices must move as one 32-bit access: the generic byte copy
    // would issue four byte writes and the card could read a torn index
    // (measured 2026-09-16: completions for tag 0 with status 0 and a hung
    // block device once the host published a head byte by byte).
    fn read_u32(&self, off: usize) -> u32 {
        assert!(off + 4 <= self.len, "ring read outside region");
        self.aperture.read32(self.base + off)
    }
    fn write_u32(&mut self, off: usize, v: u32) {
        assert!(off + 4 <= self.len, "ring write outside region");
        self.aperture.write32(self.base + off, v);
    }
    fn fence(&mut self) {
        // A PCIe read cannot complete before earlier posted writes from this
        // CPU reach the device: reading anything in the region drains them.
        let _ = self.aperture.read32(self.base);
    }
}
