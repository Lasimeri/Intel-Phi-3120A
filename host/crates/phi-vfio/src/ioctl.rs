//! VFIO UAPI constants and structures, transcribed from
//! `/usr/include/linux/vfio.h` (linux-api-headers on Arch, kernel 7.x).
//!
//! `_IO(';', 100 + n)` encodes as `(';' << 8) | (100 + n)` on x86-64 because
//! `_IO` has direction NONE and size 0. All ioctls below use that form; VFIO
//! deliberately passes sizes inside the structs (`argsz`) instead of in the
//! ioctl number.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd};

/// `VFIO_TYPE`, the ioctl type byte `';'`.
pub const VFIO_TYPE: u32 = b';' as u32;
/// `VFIO_BASE`, the first ioctl number.
pub const VFIO_BASE: u32 = 100;

/// Build a VFIO ioctl request number (`_IO(VFIO_TYPE, VFIO_BASE + n)`).
pub const fn vfio_io(n: u32) -> libc::c_ulong {
    ((VFIO_TYPE << 8) | (VFIO_BASE + n)) as libc::c_ulong
}

/// `VFIO_GET_API_VERSION`; must return [`VFIO_API_VERSION`].
pub const VFIO_GET_API_VERSION: libc::c_ulong = vfio_io(0);
/// `VFIO_CHECK_EXTENSION`; argument is an IOMMU type, returns 1 if supported.
pub const VFIO_CHECK_EXTENSION: libc::c_ulong = vfio_io(1);
/// `VFIO_SET_IOMMU`; argument is an IOMMU type.
pub const VFIO_SET_IOMMU: libc::c_ulong = vfio_io(2);
/// `VFIO_GROUP_GET_STATUS`.
pub const VFIO_GROUP_GET_STATUS: libc::c_ulong = vfio_io(3);
/// `VFIO_GROUP_SET_CONTAINER`; argument is a pointer to the container fd.
pub const VFIO_GROUP_SET_CONTAINER: libc::c_ulong = vfio_io(4);
/// `VFIO_GROUP_GET_DEVICE_FD`; argument is the device name string.
pub const VFIO_GROUP_GET_DEVICE_FD: libc::c_ulong = vfio_io(6);
/// `VFIO_DEVICE_GET_INFO`.
pub const VFIO_DEVICE_GET_INFO: libc::c_ulong = vfio_io(7);
/// `VFIO_DEVICE_GET_REGION_INFO`.
pub const VFIO_DEVICE_GET_REGION_INFO: libc::c_ulong = vfio_io(8);
/// `VFIO_DEVICE_GET_IRQ_INFO`.
pub const VFIO_DEVICE_GET_IRQ_INFO: libc::c_ulong = vfio_io(9);
/// `VFIO_DEVICE_SET_IRQS`.
pub const VFIO_DEVICE_SET_IRQS: libc::c_ulong = vfio_io(10);
/// `VFIO_DEVICE_RESET`.
pub const VFIO_DEVICE_RESET: libc::c_ulong = vfio_io(11);
/// `VFIO_IOMMU_GET_INFO`.
pub const VFIO_IOMMU_GET_INFO: libc::c_ulong = vfio_io(12);
/// `VFIO_IOMMU_MAP_DMA`.
pub const VFIO_IOMMU_MAP_DMA: libc::c_ulong = vfio_io(13);
/// `VFIO_IOMMU_UNMAP_DMA`.
pub const VFIO_IOMMU_UNMAP_DMA: libc::c_ulong = vfio_io(14);

/// Expected `VFIO_GET_API_VERSION` result.
pub const VFIO_API_VERSION: i32 = 0;
/// `VFIO_TYPE1v2_IOMMU`, the IOMMU backend this crate requires.
pub const VFIO_TYPE1V2_IOMMU: i32 = 3;

/// `VFIO_GROUP_FLAGS_VIABLE`.
pub const VFIO_GROUP_FLAGS_VIABLE: u32 = 1 << 0;
/// `VFIO_GROUP_FLAGS_CONTAINER_SET`.
pub const VFIO_GROUP_FLAGS_CONTAINER_SET: u32 = 1 << 1;

/// `VFIO_DEVICE_FLAGS_RESET`: device supports `VFIO_DEVICE_RESET`.
pub const VFIO_DEVICE_FLAGS_RESET: u32 = 1 << 0;
/// `VFIO_DEVICE_FLAGS_PCI`.
pub const VFIO_DEVICE_FLAGS_PCI: u32 = 1 << 1;
/// `VFIO_DEVICE_FLAGS_CAPS`: `cap_offset` in the device info is valid.
pub const VFIO_DEVICE_FLAGS_CAPS: u32 = 1 << 7;

/// `VFIO_REGION_INFO_FLAG_READ`.
pub const VFIO_REGION_INFO_FLAG_READ: u32 = 1 << 0;
/// `VFIO_REGION_INFO_FLAG_WRITE`.
pub const VFIO_REGION_INFO_FLAG_WRITE: u32 = 1 << 1;
/// `VFIO_REGION_INFO_FLAG_MMAP`.
pub const VFIO_REGION_INFO_FLAG_MMAP: u32 = 1 << 2;
/// `VFIO_REGION_INFO_FLAG_CAPS`: `cap_offset` in the region info is valid.
pub const VFIO_REGION_INFO_FLAG_CAPS: u32 = 1 << 3;

/// `VFIO_IRQ_INFO_EVENTFD`: the index supports eventfd signalling.
pub const VFIO_IRQ_INFO_EVENTFD: u32 = 1 << 0;

/// `VFIO_IRQ_SET_DATA_NONE`.
pub const VFIO_IRQ_SET_DATA_NONE: u32 = 1 << 0;
/// `VFIO_IRQ_SET_DATA_EVENTFD`.
pub const VFIO_IRQ_SET_DATA_EVENTFD: u32 = 1 << 2;
/// `VFIO_IRQ_SET_ACTION_TRIGGER`.
pub const VFIO_IRQ_SET_ACTION_TRIGGER: u32 = 1 << 5;

/// `VFIO_IOMMU_INFO_PGSIZES`: `iova_pgsizes` in the IOMMU info is valid.
pub const VFIO_IOMMU_INFO_PGSIZES: u32 = 1 << 0;

/// `VFIO_DMA_MAP_FLAG_READ`: device may read the mapping.
pub const VFIO_DMA_MAP_FLAG_READ: u32 = 1 << 0;
/// `VFIO_DMA_MAP_FLAG_WRITE`: device may write the mapping.
pub const VFIO_DMA_MAP_FLAG_WRITE: u32 = 1 << 1;

/// PCI region indexes (`enum` in vfio.h): BAR0..BAR5 are 0..5.
pub const VFIO_PCI_BAR0_REGION_INDEX: u32 = 0;
/// Expansion ROM region.
pub const VFIO_PCI_ROM_REGION_INDEX: u32 = 6;
/// PCI configuration space region.
pub const VFIO_PCI_CONFIG_REGION_INDEX: u32 = 7;

/// PCI IRQ indexes.
pub const VFIO_PCI_INTX_IRQ_INDEX: u32 = 0;
/// MSI.
pub const VFIO_PCI_MSI_IRQ_INDEX: u32 = 1;
/// MSI-X.
pub const VFIO_PCI_MSIX_IRQ_INDEX: u32 = 2;

/// `struct vfio_group_status`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct VfioGroupStatus {
    /// Size of this struct, filled in by the caller.
    pub argsz: u32,
    /// `VFIO_GROUP_FLAGS_*`.
    pub flags: u32,
}

/// `struct vfio_device_info`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct VfioDeviceInfo {
    /// Size of this struct.
    pub argsz: u32,
    /// `VFIO_DEVICE_FLAGS_*`.
    pub flags: u32,
    /// Max region index + 1.
    pub num_regions: u32,
    /// Max IRQ index + 1.
    pub num_irqs: u32,
    /// Offset of the first capability, if [`VFIO_DEVICE_FLAGS_CAPS`].
    pub cap_offset: u32,
    /// Padding.
    pub pad: u32,
}

/// `struct vfio_region_info`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct VfioRegionInfo {
    /// Size of this struct.
    pub argsz: u32,
    /// `VFIO_REGION_INFO_FLAG_*`.
    pub flags: u32,
    /// Region index, filled in by the caller.
    pub index: u32,
    /// Offset of the first capability, if [`VFIO_REGION_INFO_FLAG_CAPS`].
    pub cap_offset: u32,
    /// Region size in bytes.
    pub size: u64,
    /// Offset of the region within the device fd (for `pread`/`mmap`).
    pub offset: u64,
}

/// `struct vfio_irq_info`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct VfioIrqInfo {
    /// Size of this struct.
    pub argsz: u32,
    /// `VFIO_IRQ_INFO_*`.
    pub flags: u32,
    /// IRQ index, filled in by the caller.
    pub index: u32,
    /// Number of vectors at this index.
    pub count: u32,
}

/// Fixed header of `struct vfio_irq_set`; the variable `data[]` follows it.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct VfioIrqSetHeader {
    /// Size of the header plus data.
    pub argsz: u32,
    /// `VFIO_IRQ_SET_DATA_*` | `VFIO_IRQ_SET_ACTION_*`.
    pub flags: u32,
    /// IRQ index.
    pub index: u32,
    /// First vector.
    pub start: u32,
    /// Number of vectors.
    pub count: u32,
}

/// `struct vfio_iommu_type1_info`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct VfioIommuType1Info {
    /// Size of this struct.
    pub argsz: u32,
    /// `VFIO_IOMMU_INFO_*`.
    pub flags: u32,
    /// Bitmap of supported IOVA page sizes.
    pub iova_pgsizes: u64,
    /// Offset of the first capability.
    pub cap_offset: u32,
    /// Padding.
    pub pad: u32,
}

/// `struct vfio_iommu_type1_dma_map`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct VfioIommuType1DmaMap {
    /// Size of this struct.
    pub argsz: u32,
    /// `VFIO_DMA_MAP_FLAG_*`.
    pub flags: u32,
    /// Process virtual address of the memory to pin and map.
    pub vaddr: u64,
    /// IOVA the device will use.
    pub iova: u64,
    /// Length in bytes.
    pub size: u64,
}

/// `struct vfio_iommu_type1_dma_unmap` (fixed part; no dirty-bitmap data used).
/// On return the kernel overwrites `size` with the number of bytes unmapped.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct VfioIommuType1DmaUnmap {
    /// Size of this struct.
    pub argsz: u32,
    /// `VFIO_DMA_UNMAP_FLAG_*`.
    pub flags: u32,
    /// IOVA to unmap.
    pub iova: u64,
    /// Length in bytes; bytes actually unmapped on return.
    pub size: u64,
}

/// Issue an ioctl with a pointer argument. Returns the raw result.
///
/// # Safety
/// `arg` must point to a struct of the layout the request expects, with
/// `argsz` filled in where the UAPI requires it.
pub unsafe fn ioctl_ptr<T>(fd: BorrowedFd<'_>, req: libc::c_ulong, arg: *mut T) -> io::Result<libc::c_int> {
    let r = libc::ioctl(fd.as_raw_fd(), req, arg);
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

/// Issue an ioctl with an integer argument (used for `VFIO_CHECK_EXTENSION`
/// and `VFIO_SET_IOMMU`, whose argument is a plain `unsigned long`, and for
/// the argument-less `VFIO_GET_API_VERSION` and `VFIO_DEVICE_RESET`).
pub fn ioctl_int(fd: BorrowedFd<'_>, req: libc::c_ulong, arg: libc::c_ulong) -> io::Result<libc::c_int> {
    // SAFETY: an integer argument is always valid to pass; the kernel does
    // not dereference it for these requests.
    let r = unsafe { libc::ioctl(fd.as_raw_fd(), req, arg) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn ioctl_numbers_match_vfio_h() {
        // ';' is 0x3B, VFIO_BASE is 100 = 0x64; _IO adds nothing else.
        for (req, expect) in [
            (VFIO_GET_API_VERSION, 0x3B64),
            (VFIO_CHECK_EXTENSION, 0x3B65),
            (VFIO_SET_IOMMU, 0x3B66),
            (VFIO_GROUP_GET_STATUS, 0x3B67),
            (VFIO_GROUP_SET_CONTAINER, 0x3B68),
            (VFIO_GROUP_GET_DEVICE_FD, 0x3B6A),
            (VFIO_DEVICE_GET_INFO, 0x3B6B),
            (VFIO_DEVICE_GET_REGION_INFO, 0x3B6C),
            (VFIO_DEVICE_GET_IRQ_INFO, 0x3B6D),
            (VFIO_DEVICE_SET_IRQS, 0x3B6E),
            (VFIO_DEVICE_RESET, 0x3B6F),
            (VFIO_IOMMU_GET_INFO, 0x3B70),
            (VFIO_IOMMU_MAP_DMA, 0x3B71),
            (VFIO_IOMMU_UNMAP_DMA, 0x3B72),
        ] {
            assert_eq!(req, expect);
        }
    }

    #[test]
    fn struct_sizes_match_vfio_h() {
        assert_eq!(size_of::<VfioGroupStatus>(), 8);
        assert_eq!(size_of::<VfioDeviceInfo>(), 24);
        assert_eq!(size_of::<VfioRegionInfo>(), 32);
        assert_eq!(size_of::<VfioIrqInfo>(), 16);
        assert_eq!(size_of::<VfioIrqSetHeader>(), 20);
        assert_eq!(size_of::<VfioIommuType1Info>(), 24);
        assert_eq!(size_of::<VfioIommuType1DmaMap>(), 32);
        assert_eq!(size_of::<VfioIommuType1DmaUnmap>(), 24);
    }

    #[test]
    fn field_offsets_match_vfio_h() {
        // __aligned_u64 fields sit at 8-byte boundaries after the u32 pairs.
        assert_eq!(offset_of!(VfioRegionInfo, cap_offset), 12);
        assert_eq!(offset_of!(VfioRegionInfo, size), 16);
        assert_eq!(offset_of!(VfioRegionInfo, offset), 24);
        assert_eq!(offset_of!(VfioIommuType1Info, iova_pgsizes), 8);
        assert_eq!(offset_of!(VfioIommuType1Info, cap_offset), 16);
        assert_eq!(offset_of!(VfioIommuType1DmaMap, vaddr), 8);
        assert_eq!(offset_of!(VfioIommuType1DmaMap, iova), 16);
        assert_eq!(offset_of!(VfioIommuType1DmaMap, size), 24);
        assert_eq!(offset_of!(VfioIommuType1DmaUnmap, iova), 8);
        assert_eq!(offset_of!(VfioIommuType1DmaUnmap, size), 16);
        assert_eq!(offset_of!(VfioIrqSetHeader, count), 16);
        assert_eq!(offset_of!(VfioDeviceInfo, cap_offset), 16);
    }
}
