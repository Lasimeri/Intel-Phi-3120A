//! sysfs lookups: IOMMU group of a device, driver binding, locating the Phi.

use std::fs;
use std::path::Path;

use crate::{Error, Result};

const PCI_DEVICES: &str = "/sys/bus/pci/devices";

fn read_hex_u16(path: &Path) -> Result<u16> {
    let s = fs::read_to_string(path).map_err(|e| Error::Os("read sysfs", e))?;
    let s = s.trim().trim_start_matches("0x");
    u16::from_str_radix(s, 16).map_err(|_| Error::Sysfs(format!("{}: not hex: {s:?}", path.display())))
}

/// IOMMU group number of the device at `bdf` (`/sys/bus/pci/devices/<bdf>/iommu_group`).
pub fn iommu_group_of(bdf: &str) -> Result<u32> {
    let link = Path::new(PCI_DEVICES).join(bdf).join("iommu_group");
    let target = fs::read_link(&link).map_err(|e| Error::Os("readlink iommu_group (is the IOMMU enabled?)", e))?;
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Sysfs(format!("{}: bad link", link.display())))?;
    name.parse()
        .map_err(|_| Error::Sysfs(format!("iommu_group name {name:?} is not a number")))
}

/// Name of the driver bound to `bdf`, if any.
pub fn driver_of(bdf: &str) -> Option<String> {
    let link = Path::new(PCI_DEVICES).join(bdf).join("driver");
    fs::read_link(link)
        .ok()
        .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// Vendor and device ID of `bdf` from sysfs.
pub fn ids_of(bdf: &str) -> Result<(u16, u16)> {
    let d = Path::new(PCI_DEVICES).join(bdf);
    Ok((read_hex_u16(&d.join("vendor"))?, read_hex_u16(&d.join("device"))?))
}

/// Check that `bdf` is a Xeon Phi 3120-series device.
pub fn require_phi(bdf: &str) -> Result<()> {
    let (v, d) = ids_of(bdf)?;
    if v != phi_regs::PCI_VENDOR_INTEL || d != phi_regs::PCI_DEVICE_3120 {
        return Err(Error::NotPhi(bdf.to_string(), v, d));
    }
    Ok(())
}

/// Find the first Xeon Phi 3120-series device on the bus, by full BDF.
pub fn find_phi() -> Result<Option<String>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(PCI_DEVICES).map_err(|e| Error::Os("read /sys/bus/pci/devices", e))? {
        let entry = entry.map_err(|e| Error::Os("read_dir entry", e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Ok((v, d)) = ids_of(&name) {
            if v == phi_regs::PCI_VENDOR_INTEL && d == phi_regs::PCI_DEVICE_3120 {
                found.push(name);
            }
        }
    }
    found.sort();
    Ok(found.into_iter().next())
}

/// Resolve a BDF from an explicit argument, then `PHI_BDF`, then autodetection.
pub fn resolve_bdf(explicit: Option<&str>) -> Result<String> {
    if let Some(b) = explicit {
        return Ok(b.to_string());
    }
    if let Ok(b) = std::env::var("PHI_BDF") {
        if !b.is_empty() {
            return Ok(b);
        }
    }
    find_phi()?.ok_or_else(|| Error::Sysfs("no Xeon Phi 3120 series device found; pass --bdf or set PHI_BDF".into()))
}
