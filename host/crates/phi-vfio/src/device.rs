//! The VFIO device fd: regions, config space, interrupts, reset.

use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};

use crate::ioctl::*;
use crate::mapping::Mapping;
use crate::{Error, Result};

/// An open device fd.
pub struct Device {
    fd: OwnedFd,
    info: VfioDeviceInfo,
    config_offset: u64,
}

/// PCI configuration space offsets used here (PCI Local Bus spec 3.0, 6.1).
pub mod pci_cfg {
    /// Vendor ID (u16).
    pub const VENDOR_ID: u64 = 0x00;
    /// Device ID (u16).
    pub const DEVICE_ID: u64 = 0x02;
    /// Command register (u16).
    pub const COMMAND: u64 = 0x04;
    /// Memory Space Enable bit in COMMAND.
    pub const COMMAND_MEMORY: u16 = 1 << 1;
    /// Bus Master Enable bit in COMMAND.
    pub const COMMAND_BUS_MASTER: u16 = 1 << 2;
    /// Subsystem vendor ID (u16).
    pub const SUBSYSTEM_VENDOR_ID: u64 = 0x2C;
    /// Subsystem device ID (u16).
    pub const SUBSYSTEM_ID: u64 = 0x2E;
}

impl Device {
    /// Wrap a device fd obtained from a group and query its info.
    pub(crate) fn from_fd(fd: OwnedFd) -> Result<Self> {
        let mut info = VfioDeviceInfo {
            argsz: std::mem::size_of::<VfioDeviceInfo>() as u32,
            ..Default::default()
        };
        // SAFETY: correctly sized struct with argsz set.
        unsafe { ioctl_ptr(fd.as_fd(), VFIO_DEVICE_GET_INFO, &mut info) }.map_err(|e| Error::Os("VFIO_DEVICE_GET_INFO", e))?;
        let mut dev = Self {
            fd,
            info,
            config_offset: 0,
        };
        let cfg = dev.region_info(VFIO_PCI_CONFIG_REGION_INDEX)?;
        dev.config_offset = cfg.offset;
        Ok(dev)
    }

    /// `VFIO_DEVICE_GET_INFO` result.
    pub fn info(&self) -> &VfioDeviceInfo {
        &self.info
    }

    /// Query one region (BAR index 0..5, ROM 6, config 7).
    pub fn region_info(&self, index: u32) -> Result<VfioRegionInfo> {
        let mut ri = VfioRegionInfo {
            argsz: std::mem::size_of::<VfioRegionInfo>() as u32,
            index,
            ..Default::default()
        };
        // SAFETY: correctly sized struct with argsz and index set.
        unsafe { ioctl_ptr(self.fd.as_fd(), VFIO_DEVICE_GET_REGION_INFO, &mut ri) }
            .map_err(|e| Error::Os("VFIO_DEVICE_GET_REGION_INFO", e))?;
        Ok(ri)
    }

    /// mmap a whole region. Fails if the region lacks the MMAP flag (config
    /// space and the ROM are read with `pread` instead).
    pub fn map_region(&self, index: u32) -> Result<Mapping> {
        let ri = self.region_info(index)?;
        if ri.flags & VFIO_REGION_INFO_FLAG_MMAP == 0 || ri.size == 0 {
            return Err(Error::NotMmappable(index));
        }
        Mapping::new(self.fd.as_fd(), ri.offset, ri.size as usize).map_err(|e| Error::Os("mmap region", e))
    }

    /// Read `buf.len()` bytes of PCI config space at `offset`.
    pub fn read_config(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        // SAFETY: pread into a valid buffer at a valid fd offset.
        let n = unsafe {
            libc::pread(
                self.fd.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                (self.config_offset + offset) as libc::off_t,
            )
        };
        if n != buf.len() as isize {
            return Err(Error::Os("pread config space", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Write `data` to PCI config space at `offset`. VFIO virtualizes some
    /// bits (BARs, MSI-X capability); the COMMAND register memory and bus
    /// master bits pass through to hardware.
    pub fn write_config(&self, offset: u64, data: &[u8]) -> Result<()> {
        // SAFETY: pwrite from a valid buffer.
        let n = unsafe {
            libc::pwrite(
                self.fd.as_raw_fd(),
                data.as_ptr() as *const libc::c_void,
                data.len(),
                (self.config_offset + offset) as libc::off_t,
            )
        };
        if n != data.len() as isize {
            return Err(Error::Os("pwrite config space", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Read a 16-bit config register.
    pub fn read_config_u16(&self, offset: u64) -> Result<u16> {
        let mut b = [0u8; 2];
        self.read_config(offset, &mut b)?;
        Ok(u16::from_le_bytes(b))
    }

    /// Set Memory Space Enable and Bus Master Enable in COMMAND. Without
    /// memory decode the BARs do not respond; `vfio-pci` enables the device
    /// on open in recent kernels, but being explicit costs nothing.
    pub fn enable_memory_and_bus_master(&self) -> Result<u16> {
        let cmd = self.read_config_u16(pci_cfg::COMMAND)?;
        let want = cmd | pci_cfg::COMMAND_MEMORY | pci_cfg::COMMAND_BUS_MASTER;
        if want != cmd {
            self.write_config(pci_cfg::COMMAND, &want.to_le_bytes())?;
        }
        self.read_config_u16(pci_cfg::COMMAND)
    }

    /// Vendor, device, subsystem vendor, subsystem device IDs.
    pub fn ids(&self) -> Result<(u16, u16, u16, u16)> {
        Ok((
            self.read_config_u16(pci_cfg::VENDOR_ID)?,
            self.read_config_u16(pci_cfg::DEVICE_ID)?,
            self.read_config_u16(pci_cfg::SUBSYSTEM_VENDOR_ID)?,
            self.read_config_u16(pci_cfg::SUBSYSTEM_ID)?,
        ))
    }

    /// Query an IRQ index (INTx 0, MSI 1, MSI-X 2): vector count and flags.
    pub fn irq_info(&self, index: u32) -> Result<VfioIrqInfo> {
        let mut ii = VfioIrqInfo {
            argsz: std::mem::size_of::<VfioIrqInfo>() as u32,
            index,
            ..Default::default()
        };
        // SAFETY: correctly sized struct with argsz and index set.
        unsafe { ioctl_ptr(self.fd.as_fd(), VFIO_DEVICE_GET_IRQ_INFO, &mut ii) }.map_err(|e| Error::Os("VFIO_DEVICE_GET_IRQ_INFO", e))?;
        Ok(ii)
    }

    /// Route vectors `start..start+eventfds.len()` of IRQ index `index` to
    /// the given eventfds (`VFIO_IRQ_SET_DATA_EVENTFD | ACTION_TRIGGER`).
    /// Enables the interrupt mode (MSI-X allocation happens here).
    pub fn set_irq_eventfds(&self, index: u32, start: u32, eventfds: &[RawFd]) -> Result<()> {
        let hdr = VfioIrqSetHeader {
            argsz: (std::mem::size_of::<VfioIrqSetHeader>() + eventfds.len() * 4) as u32,
            flags: VFIO_IRQ_SET_DATA_EVENTFD | VFIO_IRQ_SET_ACTION_TRIGGER,
            index,
            start,
            count: eventfds.len() as u32,
        };
        let mut buf = Vec::with_capacity(hdr.argsz as usize);
        // SAFETY: VfioIrqSetHeader is repr(C) plain data; viewing it as bytes is sound.
        buf.extend_from_slice(unsafe {
            std::slice::from_raw_parts(&hdr as *const _ as *const u8, std::mem::size_of::<VfioIrqSetHeader>())
        });
        for fd in eventfds {
            buf.extend_from_slice(&fd.to_le_bytes());
        }
        // SAFETY: buffer laid out exactly as struct vfio_irq_set with data[].
        unsafe { ioctl_ptr(self.fd.as_fd(), VFIO_DEVICE_SET_IRQS, buf.as_mut_ptr()) }.map_err(|e| Error::Os("VFIO_DEVICE_SET_IRQS", e))?;
        Ok(())
    }

    /// Disable all vectors of IRQ index `index` (`DATA_NONE | ACTION_TRIGGER`, count 0).
    pub fn disable_irqs(&self, index: u32) -> Result<()> {
        let mut hdr = VfioIrqSetHeader {
            argsz: std::mem::size_of::<VfioIrqSetHeader>() as u32,
            flags: VFIO_IRQ_SET_DATA_NONE | VFIO_IRQ_SET_ACTION_TRIGGER,
            index,
            start: 0,
            count: 0,
        };
        // SAFETY: header-only struct is a valid vfio_irq_set with no data.
        unsafe { ioctl_ptr(self.fd.as_fd(), VFIO_DEVICE_SET_IRQS, &mut hdr) }
            .map_err(|e| Error::Os("VFIO_DEVICE_SET_IRQS (disable)", e))?;
        Ok(())
    }

    /// Function-level or bus reset via VFIO. The Phi has no FLR; VFIO falls
    /// back to a secondary bus reset, which is allowed because the card is
    /// alone on its bus. Prefer the SBOX `RGCR` reset in normal operation.
    pub fn reset(&self) -> Result<()> {
        ioctl_int(self.fd.as_fd(), VFIO_DEVICE_RESET, 0).map_err(|e| Error::Os("VFIO_DEVICE_RESET", e))?;
        Ok(())
    }

    /// The raw fd.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}
