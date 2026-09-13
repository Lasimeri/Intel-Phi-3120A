//! The VFIO group: `/dev/vfio/<N>`, one IOMMU group.

use std::ffi::CString;
use std::fs::OpenOptions;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};

use crate::container::Container;
use crate::device::Device;
use crate::ioctl::*;
use crate::{Error, Result};

/// An open group fd.
pub struct Group {
    fd: OwnedFd,
    id: u32,
}

impl Group {
    /// Open `/dev/vfio/<id>` and verify the group is viable.
    pub fn open(id: u32) -> Result<Self> {
        let path = format!("/dev/vfio/{id}");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| Error::Os("open /dev/vfio/<group>", e))?;
        let fd: OwnedFd = file.into();
        let mut st = VfioGroupStatus {
            argsz: std::mem::size_of::<VfioGroupStatus>() as u32,
            flags: 0,
        };
        // SAFETY: correctly sized struct with argsz set.
        unsafe { ioctl_ptr(fd.as_fd(), VFIO_GROUP_GET_STATUS, &mut st) }.map_err(|e| Error::Os("VFIO_GROUP_GET_STATUS", e))?;
        if st.flags & VFIO_GROUP_FLAGS_VIABLE == 0 {
            return Err(Error::GroupNotViable(id));
        }
        Ok(Self { fd, id })
    }

    /// Group number.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Attach this group to a container (`VFIO_GROUP_SET_CONTAINER`).
    pub fn set_container(&self, c: &Container) -> Result<()> {
        use std::os::fd::AsRawFd;
        let mut cfd: libc::c_int = c.fd().as_raw_fd();
        // SAFETY: the argument is a pointer to an int holding the container fd, as the UAPI specifies.
        unsafe { ioctl_ptr(self.fd.as_fd(), VFIO_GROUP_SET_CONTAINER, &mut cfd) }.map_err(|e| Error::Os("VFIO_GROUP_SET_CONTAINER", e))?;
        Ok(())
    }

    /// Obtain the device fd for `bdf` (`VFIO_GROUP_GET_DEVICE_FD`).
    pub fn get_device(&self, bdf: &str) -> Result<Device> {
        let name = CString::new(bdf).map_err(|_| Error::Sysfs(format!("bad BDF string {bdf:?}")))?;
        // SAFETY: the argument is a NUL-terminated device name; the kernel
        // copies it and returns a new fd, which we take ownership of.
        let fd = unsafe { ioctl_ptr(self.fd.as_fd(), VFIO_GROUP_GET_DEVICE_FD, name.as_ptr() as *mut libc::c_char) }
            .map_err(|e| Error::Os("VFIO_GROUP_GET_DEVICE_FD", e))?;
        // SAFETY: a fresh fd owned by us.
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        Device::from_fd(owned)
    }
}
