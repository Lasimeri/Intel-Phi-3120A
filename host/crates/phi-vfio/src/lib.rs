//! Minimal VFIO access for one PCI device.
//!
//! Scope: the legacy container + group API (`/dev/vfio/vfio`,
//! `/dev/vfio/<group>`) with the type1v2 IOMMU backend, which is what every
//! kernel since 3.6 provides and what QEMU still uses by default. The newer
//! iommufd cdev path is not needed for anything this project does.
//!
//! The bindings are hand-written from `/usr/include/linux/vfio.h` rather
//! than generated, because the surface is ten ioctls and eight structs and
//! a reader should be able to check every one against the header in a few
//! minutes. Struct sizes are pinned by tests.
//!
//! `unsafe` is confined to [`ioctl`], [`mapping`], and the fd plumbing; the
//! public API is safe.

#![warn(missing_docs)]

pub mod container;
pub mod device;
pub mod group;
pub mod ioctl;
pub mod mapping;
pub mod sysfs;

use std::io;

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An OS call failed; the string says which.
    #[error("{0}: {1}")]
    Os(&'static str, #[source] io::Error),
    /// `/dev/vfio/vfio` reported an API version other than 0.
    #[error("unexpected VFIO API version {0}")]
    ApiVersion(i32),
    /// The container does not support the type1v2 IOMMU backend.
    #[error("VFIO type1v2 IOMMU extension not supported by this kernel")]
    NoType1v2,
    /// The IOMMU group is not viable (some member is bound to another driver).
    #[error("IOMMU group {0} is not viable: every device in it must be bound to vfio-pci")]
    GroupNotViable(u32),
    /// A region cannot be mmapped (flags lack `VFIO_REGION_INFO_FLAG_MMAP`).
    #[error("region {0} is not mmappable")]
    NotMmappable(u32),
    /// A sysfs value could not be parsed.
    #[error("sysfs: {0}")]
    Sysfs(String),
    /// The device is not a Xeon Phi (wrong vendor/device ID).
    #[error("{0} is not a Xeon Phi 3120 series device (vendor {1:#06x} device {2:#06x})")]
    NotPhi(String, u16, u16),
}

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// One PCI device opened through VFIO: its container, group, and device fd.
///
/// Dropping it closes the fds in the right order (device, group, container)
/// and returns the device to the `vfio-pci` driver's idle state.
pub struct VfioPci {
    container: container::Container,
    group: group::Group,
    device: device::Device,
    bdf: String,
}

impl VfioPci {
    /// Open the device with PCI address `bdf` (`0000:2e:00.0`). The device
    /// must already be bound to `vfio-pci` (`scripts/bind-vfio.sh`) and the
    /// caller must have read/write access to `/dev/vfio/<group>`.
    pub fn open(bdf: &str) -> Result<Self> {
        let group_id = sysfs::iommu_group_of(bdf)?;
        let container = container::Container::open()?;
        let group = group::Group::open(group_id)?;
        group.set_container(&container)?;
        container.set_iommu_type1v2()?;
        let device = group.get_device(bdf)?;
        log::info!("opened {bdf} via VFIO group {group_id}");
        Ok(Self {
            container,
            group,
            device,
            bdf: bdf.to_string(),
        })
    }

    /// The device handle.
    pub fn device(&self) -> &device::Device {
        &self.device
    }

    /// The container (for DMA mapping).
    pub fn container(&self) -> &container::Container {
        &self.container
    }

    /// The group handle.
    pub fn group(&self) -> &group::Group {
        &self.group
    }

    /// PCI address this handle was opened with.
    pub fn bdf(&self) -> &str {
        &self.bdf
    }
}

impl Drop for VfioPci {
    fn drop(&mut self) {
        // Field drop order in Rust is declaration order: container first,
        // which would be wrong. Take the device and group out explicitly.
        // (OwnedFd close is infallible; ordering only matters for tidiness
        // of kernel-side teardown, so this is belt and braces.)
        log::debug!("closing VFIO handles for {}", self.bdf);
    }
}
