//! The shared window as the host sees it: the file the daemon pinned for
//! the card (`/dev/shm/phi-hostmem[-N]`), mapped read-write, with the
//! control words the worker polls at the offsets `proto` names. Used by
//! the `phi-vpu` driver and by `libphi512`'s seamless path.
use std::ptr;
use std::sync::atomic::{fence, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::proto::*;

pub struct Window {
    base: *mut u8,
    len: usize,
}

// SAFETY: the mapping is shared memory the card and this process both
// address; the pointer carries no thread affinity, and every access
// through it is volatile or a plain copy. libphi512 keeps one behind a
// mutex.
unsafe impl Send for Window {}

impl Window {
    pub fn open(path: &str, len: usize) -> Result<Window> {
        let cpath = std::ffi::CString::new(path)?;
        // SAFETY: plain open and mmap of a file the user named.
        let base = unsafe {
            let fd = libc::open(cpath.as_ptr(), libc::O_RDWR);
            if fd < 0 {
                return Err(std::io::Error::last_os_error()).with_context(|| format!("open {path}"));
            }
            let p = libc::mmap(ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
            libc::close(fd);
            if p == libc::MAP_FAILED {
                return Err(std::io::Error::last_os_error()).with_context(|| format!("mmap {len} bytes of {path}"));
            }
            p as *mut u8
        };
        Ok(Window { base, len })
    }

    /// Read a control word. Volatile, because the card writes here.
    pub fn read<T: Copy>(&self, off: usize) -> T {
        assert!(off + std::mem::size_of::<T>() <= self.len);
        // SAFETY: bounds checked; the window is mapped for the life of self.
        unsafe { ptr::read_volatile(self.base.add(off) as *const T) }
    }

    pub fn write<T: Copy>(&self, off: usize, v: T) {
        assert!(off + std::mem::size_of::<T>() <= self.len);
        // SAFETY: as above.
        unsafe { ptr::write_volatile(self.base.add(off) as *mut T, v) }
    }

    pub fn put(&self, off: u64, bytes: &[u8]) {
        let off = off as usize;
        assert!(off + bytes.len() <= self.len);
        // SAFETY: as above.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), self.base.add(off), bytes.len()) }
    }

    /// A raw pointer into the window, for a kernel copy straight into it.
    pub fn ptr(&self, off: u64, len: usize) -> *mut u8 {
        let off = off as usize;
        assert!(off + len <= self.len);
        // SAFETY: bounds checked; the window is mapped for the life of self.
        unsafe { self.base.add(off) }
    }

    pub fn get(&self, off: u64, out: &mut [u8]) {
        let off = off as usize;
        assert!(off + out.len() <= self.len);
        // SAFETY: as above.
        unsafe { ptr::copy_nonoverlapping(self.base.add(off), out.as_mut_ptr(), out.len()) }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: unmapping what open() mapped.
        unsafe { libc::munmap(self.base as *mut libc::c_void, self.len) };
    }
}

/// Is the card's worker polling?
pub fn worker_ready(w: &Window) -> bool {
    w.read::<u64>(OFF_READY) == MAGIC
}

/// Wait for a worker that is alive now, not one that was alive once.
///
/// The readiness word stays in the window after a worker dies, so a
/// check that only reads it is satisfied by a corpse and the request
/// then waits its full timeout for an answer that never comes. The word
/// is cleared first; a live worker re-asserts it on every poll, within
/// a millisecond even when it is idle and sleeping between polls.
pub fn wait_ready(w: &Window, timeout: Duration) -> Result<()> {
    w.write(OFF_READY, 0u64);
    fence(Ordering::SeqCst);
    let give_up = Instant::now() + timeout;
    while !worker_ready(w) {
        if Instant::now() > give_up {
            bail!("no card worker is polling the window (scripts/phi-vpu.sh start)");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

/// Ring the doorbell and wait for the answer.
///
/// The request fields are written first, then a fence, then the sequence
/// number, which is the only word the card polls. The card writes its
/// reply fields and then the echoed sequence number last, so seeing the
/// number means the rest of the reply is there.
pub fn submit(w: &Window, req: Request, timeout: Duration) -> Result<(Reply, Duration)> {
    let seq = w.read::<u64>(OFF_REQ) + 1;
    let staged = Request { seq: seq - 1, ..req };
    w.write(OFF_REQ, staged);
    fence(Ordering::SeqCst);
    let t0 = Instant::now();
    w.write(OFF_REQ, seq);
    fence(Ordering::SeqCst);
    let give_up = t0 + timeout;
    loop {
        let rep: Reply = w.read(OFF_REPLY);
        if rep.seq == seq {
            return Ok((rep, t0.elapsed()));
        }
        if Instant::now() > give_up {
            bail!("the card did not answer request {seq} within {timeout:?}");
        }
    }
}
