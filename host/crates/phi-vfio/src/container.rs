//! The VFIO container: `/dev/vfio/vfio`, one IOMMU domain shared by every
//! group attached to it.

use std::fs::OpenOptions;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use crate::ioctl::*;
use crate::{Error, Result};

/// An open container fd.
pub struct Container {
    fd: OwnedFd,
}

impl Container {
    /// Open `/dev/vfio/vfio`, check the API version and that the type1v2
    /// backend exists (`VFIO_CHECK_EXTENSION` is legal before any group is
    /// attached; `VFIO_SET_IOMMU` is not).
    pub fn open() -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/vfio/vfio")
            .map_err(|e| Error::Os("open /dev/vfio/vfio", e))?;
        let fd: OwnedFd = file.into();
        let ver = ioctl_int(fd.as_fd(), VFIO_GET_API_VERSION, 0).map_err(|e| Error::Os("VFIO_GET_API_VERSION", e))?;
        if ver != VFIO_API_VERSION {
            return Err(Error::ApiVersion(ver));
        }
        let ext = ioctl_int(fd.as_fd(), VFIO_CHECK_EXTENSION, VFIO_TYPE1V2_IOMMU as libc::c_ulong)
            .map_err(|e| Error::Os("VFIO_CHECK_EXTENSION", e))?;
        if ext != 1 {
            return Err(Error::NoType1v2);
        }
        Ok(Self { fd })
    }

    /// Select the type1v2 IOMMU backend. Legal only after a group has been
    /// attached (`VFIO_SET_IOMMU` returns `EINVAL` on an empty container).
    pub fn set_iommu_type1v2(&self) -> Result<()> {
        ioctl_int(self.fd.as_fd(), VFIO_SET_IOMMU, VFIO_TYPE1V2_IOMMU as libc::c_ulong).map_err(|e| Error::Os("VFIO_SET_IOMMU", e))?;
        Ok(())
    }

    /// Supported IOVA page sizes as a bitmap (bit n set means 2^n bytes).
    /// Legal only after [`Self::set_iommu_type1v2`]; type1 always sets
    /// `VFIO_IOMMU_INFO_PGSIZES`, and a kernel that did not would return 0
    /// here, which the caller should treat as "4 KiB only".
    pub fn iova_page_sizes(&self) -> Result<u64> {
        let mut info = VfioIommuType1Info {
            argsz: std::mem::size_of::<VfioIommuType1Info>() as u32,
            ..Default::default()
        };
        // SAFETY: correctly sized struct with argsz set.
        unsafe { ioctl_ptr(self.fd.as_fd(), VFIO_IOMMU_GET_INFO, &mut info) }.map_err(|e| Error::Os("VFIO_IOMMU_GET_INFO", e))?;
        Ok(if info.flags & VFIO_IOMMU_INFO_PGSIZES != 0 {
            info.iova_pgsizes
        } else {
            0
        })
    }

    /// Pin `len` bytes of this process's memory at `vaddr` and map them at
    /// `iova` for device access, readable and writable by the device. The
    /// memory stays pinned until [`Self::unmap_dma`] or the container is
    /// closed; munmapping it first leaves the pages pinned and the IOVA
    /// occupied (a second `map_dma` there fails with `EEXIST`).
    ///
    /// The kernel requires `vaddr`, `iova` and `len` to be multiples of the
    /// smallest supported IOVA page (4 KiB here) and rejects a range that
    /// overlaps an existing mapping or a reserved region
    /// (`docs/hardware.md`) with `EINVAL` or `EEXIST`.
    ///
    /// # Safety
    /// `vaddr..vaddr+len` must be a valid, page-aligned mapping owned by the
    /// caller that outlives the DMA mapping; the device may write to it.
    pub unsafe fn map_dma(&self, vaddr: *mut u8, iova: u64, len: u64) -> Result<()> {
        let mut m = VfioIommuType1DmaMap {
            argsz: std::mem::size_of::<VfioIommuType1DmaMap>() as u32,
            flags: VFIO_DMA_MAP_FLAG_READ | VFIO_DMA_MAP_FLAG_WRITE,
            vaddr: vaddr as u64,
            iova,
            size: len,
        };
        ioctl_ptr(self.fd.as_fd(), VFIO_IOMMU_MAP_DMA, &mut m).map_err(|e| Error::Os("VFIO_IOMMU_MAP_DMA", e))?;
        Ok(())
    }

    /// Remove the mappings made by [`Self::map_dma`] inside `iova..iova+len`.
    /// type1v2 unmaps whole mappings only, so the range must be the union
    /// of earlier `map_dma` ranges; the kernel reports how many bytes it
    /// unmapped and this returns [`Error::PartialUnmap`] when that differs
    /// from `len` (the mappings, and the pinned memory, are then still
    /// partly in place).
    pub fn unmap_dma(&self, iova: u64, len: u64) -> Result<()> {
        let mut u = VfioIommuType1DmaUnmap {
            argsz: std::mem::size_of::<VfioIommuType1DmaUnmap>() as u32,
            flags: 0,
            iova,
            size: len,
        };
        // SAFETY: correctly sized struct with argsz set.
        unsafe { ioctl_ptr(self.fd.as_fd(), VFIO_IOMMU_UNMAP_DMA, &mut u) }.map_err(|e| Error::Os("VFIO_IOMMU_UNMAP_DMA", e))?;
        if u.size != len {
            return Err(Error::PartialUnmap {
                iova,
                requested: len,
                unmapped: u.size,
            });
        }
        Ok(())
    }

    /// The raw fd, for `VFIO_GROUP_SET_CONTAINER`.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}
