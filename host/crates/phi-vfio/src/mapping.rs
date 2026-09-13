//! A memory-mapped device region with volatile accessors.
//!
//! VFIO maps PCI BARs uncached (`pgprot_noncached`). Reads and writes must
//! be volatile so the compiler neither elides nor reorders them, and must
//! use the access width the device expects: the SBOX registers are 32-bit,
//! the aperture accepts any width. Nothing here caches or buffers.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd};
use std::ptr::{self, NonNull};

/// An mmapped region of the VFIO device fd.
pub struct Mapping {
    base: NonNull<u8>,
    len: usize,
}

// SAFETY: the mapping is plain memory owned by this process; concurrent
// volatile access from multiple threads is the device's problem, not a
// memory-safety problem, and the public API takes `&self` for reads and
// `&self` for writes deliberately (device registers are shared state).
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    /// mmap `len` bytes of `fd` starting at `offset`, read/write, shared.
    pub fn new(fd: BorrowedFd<'_>, offset: u64, len: usize) -> io::Result<Self> {
        // SAFETY: mmap with a valid fd and length; the result is checked.
        let p = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                offset as libc::off_t,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            base: NonNull::new(p as *mut u8).expect("mmap returned null"),
            len,
        })
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True if the mapping is empty (never, for a real BAR).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn check(&self, off: usize, width: usize) {
        assert!(
            off.checked_add(width).is_some_and(|end| end <= self.len),
            "access at {off:#x}+{width} outside mapping of {:#x} bytes",
            self.len
        );
    }

    /// Volatile 32-bit read at byte offset `off` (must be 4-aligned).
    pub fn read32(&self, off: usize) -> u32 {
        self.check(off, 4);
        assert!(off.is_multiple_of(4), "unaligned 32-bit read at {off:#x}");
        // SAFETY: bounds and alignment checked above.
        unsafe { ptr::read_volatile(self.base.as_ptr().add(off) as *const u32) }
    }

    /// Volatile 32-bit write at byte offset `off` (must be 4-aligned).
    pub fn write32(&self, off: usize, v: u32) {
        self.check(off, 4);
        assert!(off.is_multiple_of(4), "unaligned 32-bit write at {off:#x}");
        // SAFETY: bounds and alignment checked above.
        unsafe { ptr::write_volatile(self.base.as_ptr().add(off) as *mut u32, v) }
    }

    /// Volatile 8-bit read.
    pub fn read8(&self, off: usize) -> u8 {
        self.check(off, 1);
        // SAFETY: bounds checked.
        unsafe { ptr::read_volatile(self.base.as_ptr().add(off)) }
    }

    /// Volatile 8-bit write.
    pub fn write8(&self, off: usize, v: u8) {
        self.check(off, 1);
        // SAFETY: bounds checked.
        unsafe { ptr::write_volatile(self.base.as_ptr().add(off), v) }
    }

    /// Copy `data` into the mapping at `off` using 8-byte volatile stores
    /// where alignment allows and byte stores for the ragged edges. Meant
    /// for the aperture (plain memory behind a BAR), not for registers.
    pub fn write_bytes(&self, off: usize, data: &[u8]) {
        self.check(off, data.len());
        let mut i = 0;
        // Head: bytes until 8-aligned.
        while i < data.len() && !(off + i).is_multiple_of(8) {
            self.write8(off + i, data[i]);
            i += 1;
        }
        // Body: 8 bytes at a time.
        while i + 8 <= data.len() {
            let v = u64::from_le_bytes(data[i..i + 8].try_into().unwrap());
            // SAFETY: bounds checked by `check`; alignment established by the head loop.
            unsafe { ptr::write_volatile(self.base.as_ptr().add(off + i) as *mut u64, v) }
            i += 8;
        }
        // Tail.
        while i < data.len() {
            self.write8(off + i, data[i]);
            i += 1;
        }
    }

    /// Copy `buf.len()` bytes out of the mapping at `off`, mirror of [`Self::write_bytes`].
    pub fn read_bytes(&self, off: usize, buf: &mut [u8]) {
        self.check(off, buf.len());
        let mut i = 0;
        while i < buf.len() && !(off + i).is_multiple_of(8) {
            buf[i] = self.read8(off + i);
            i += 1;
        }
        while i + 8 <= buf.len() {
            // SAFETY: bounds checked; aligned by the head loop.
            let v = unsafe { ptr::read_volatile(self.base.as_ptr().add(off + i) as *const u64) };
            buf[i..i + 8].copy_from_slice(&v.to_le_bytes());
            i += 8;
        }
        while i < buf.len() {
            buf[i] = self.read8(off + i);
            i += 1;
        }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: base/len came from a successful mmap.
        unsafe {
            libc::munmap(self.base.as_ptr() as *mut libc::c_void, self.len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::OwnedFd;

    /// Anonymous memory through memfd stands in for a BAR so the byte
    /// copy paths (alignment head/body/tail) are tested on plain RAM.
    fn memfd(len: usize) -> OwnedFd {
        use std::os::fd::FromRawFd;
        // SAFETY: memfd_create with a static name; result checked.
        let fd = unsafe { libc::memfd_create(c"phi-vfio-test".as_ptr(), 0) };
        assert!(fd >= 0);
        // SAFETY: fd is valid.
        assert_eq!(unsafe { libc::ftruncate(fd, len as libc::off_t) }, 0);
        // SAFETY: we own fd.
        unsafe { OwnedFd::from_raw_fd(fd) }
    }

    #[test]
    fn byte_copies_round_trip_at_every_alignment() {
        use std::os::fd::AsFd;
        let fd = memfd(4096);
        let m = Mapping::new(fd.as_fd(), 0, 4096).unwrap();
        for start in 0..17usize {
            let data: Vec<u8> = (0..37u8).map(|b| b.wrapping_mul(7).wrapping_add(start as u8)).collect();
            m.write_bytes(100 + start, &data);
            let mut back = vec![0u8; data.len()];
            m.read_bytes(100 + start, &mut back);
            assert_eq!(back, data, "alignment offset {start}");
        }
        m.write32(0, 0xDEAD_BEEF);
        assert_eq!(m.read32(0), 0xDEAD_BEEF);
        assert_eq!(m.read8(0), 0xEF, "little-endian byte order");
    }

    #[test]
    #[should_panic(expected = "outside mapping")]
    fn out_of_bounds_panics() {
        use std::os::fd::AsFd;
        let fd = memfd(4096);
        let m = Mapping::new(fd.as_fd(), 0, 4096).unwrap();
        m.read32(4096);
    }
}
