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
//! minutes. Struct sizes and field offsets are pinned by tests.
//!
//! `unsafe` is confined to [`ioctl`], [`mapping`], and the fd plumbing; the
//! public API is safe except [`container::Container::map_dma`], whose
//! contract (the device gains write access to the memory) cannot be
//! checked here.

#![warn(missing_docs)]

pub mod cards;
pub mod container;
pub mod device;
pub mod group;
pub mod ioctl;
pub mod mapping;
pub mod sysfs;
pub mod traffic;

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
    /// A PCI address string is malformed.
    #[error("bad PCI address {0:?}: expected [dddd:]bb:dd.f, hex digits, function 0..7")]
    BadBdf(String),
    /// The device is not a Xeon Phi (wrong vendor/device ID).
    #[error("{0} is not a Xeon Phi 3120 series device (vendor {1:#06x} device {2:#06x})")]
    NotPhi(String, u16, u16),
    /// A PCI config space access lies outside the config region.
    #[error("config space access at {offset:#x}+{len} outside the {size}-byte region")]
    ConfigRange {
        /// Requested offset.
        offset: u64,
        /// Requested length.
        len: usize,
        /// Region size reported by VFIO.
        size: u64,
    },
    /// `VFIO_IOMMU_UNMAP_DMA` removed a different number of bytes than
    /// asked for: the range did not correspond to earlier `map_dma` calls.
    #[error("unmap at iova {iova:#x}: asked for {requested:#x} bytes, kernel unmapped {unmapped:#x}")]
    PartialUnmap {
        /// Start of the range.
        iova: u64,
        /// Bytes requested.
        requested: u64,
        /// Bytes the kernel reports it unmapped.
        unmapped: u64,
    },
}

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// One PCI device opened through VFIO: its container, group, and device fd.
///
/// Dropping it closes the fds in the reverse order of opening (device,
/// group, container), which returns the device to the `vfio-pci` driver's
/// idle state and releases every DMA mapping made through the container.
pub struct VfioPci {
    // Rust drops fields in declaration order, so the order here is the
    // teardown order the kernel documentation describes: device fd first,
    // then the group (which detaches from the container), then the
    // container itself. The kernel tolerates any order through reference
    // counts; this keeps the teardown readable in `strace`.
    device: device::Device,
    group: group::Group,
    container: container::Container,
    bdf: String,
}

impl VfioPci {
    /// Open the device with PCI address `bdf` (`0000:2e:00.0`; the domain
    /// may be omitted). The device must already be bound to `vfio-pci`
    /// (`scripts/bind-vfio.sh`) and the caller must have read/write access
    /// to `/dev/vfio/<group>`.
    pub fn open(bdf: &str) -> Result<Self> {
        let bdf = sysfs::normalize_bdf(bdf)?;
        let group_id = sysfs::iommu_group_of(&bdf)?;
        let container = container::Container::open()?;
        let group = group::Group::open(group_id)?;
        group.set_container(&container)?;
        container.set_iommu_type1v2()?;
        let device = group.get_device(&bdf)?;
        log::info!("opened {bdf} via VFIO group {group_id}");
        Ok(Self {
            device,
            group,
            container,
            bdf,
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

    /// PCI address this handle was opened with, normalized (`0000:2e:00.0`).
    pub fn bdf(&self) -> &str {
        &self.bdf
    }
}

impl Drop for VfioPci {
    fn drop(&mut self) {
        // The fields close themselves in declaration order; this only logs.
        log::debug!("closing VFIO handles for {}", self.bdf);
    }
}
