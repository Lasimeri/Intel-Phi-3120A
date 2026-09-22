//! sysfs lookups: IOMMU group of a device, driver binding, locating the Phi.
//!
//! Every function takes a PCI address and normalizes it first
//! ([`normalize_bdf`]), so `2e:00.0` and `0000:2E:00.0` name the same
//! sysfs directory. Paths follow `Documentation/ABI/testing/sysfs-bus-pci`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

const PCI_DEVICES: &str = "/sys/bus/pci/devices";

/// Canonical form of a PCI address: `dddd:bb:dd.f`, lowercase, the domain
/// defaulting to 0000 when omitted. Rejects anything that is not hex
/// digits in that shape, a device above 31 or a function above 7.
pub fn normalize_bdf(s: &str) -> Result<String> {
    let bad = || Error::BadBdf(s.to_string());
    let s = s.trim();
    let (domain, bus, dev_fn) = match s.split(':').collect::<Vec<_>>()[..] {
        [bus, dev_fn] => ("0000", bus, dev_fn),
        [domain, bus, dev_fn] => (domain, bus, dev_fn),
        _ => return Err(bad()),
    };
    let (dev, func) = dev_fn.split_once('.').ok_or_else(bad)?;
    if domain.len() != 4 || bus.len() != 2 || dev.len() != 2 || func.len() != 1 {
        return Err(bad());
    }
    let hex = |field: &str| u32::from_str_radix(field, 16).map_err(|_| bad());
    let (domain, bus, dev, func) = (hex(domain)?, hex(bus)?, hex(dev)?, hex(func)?);
    if dev > 31 || func > 7 {
        return Err(bad());
    }
    Ok(format!("{domain:04x}:{bus:02x}:{dev:02x}.{func}"))
}

/// The device's sysfs directory, after checking that it exists.
fn device_dir(bdf: &str) -> Result<PathBuf> {
    let bdf = normalize_bdf(bdf)?;
    let dir = Path::new(PCI_DEVICES).join(&bdf);
    if !dir.is_dir() {
        return Err(Error::Sysfs(format!("no PCI device {bdf} under {PCI_DEVICES}")));
    }
    Ok(dir)
}

fn read_hex_u16(path: &Path) -> Result<u16> {
    let s = fs::read_to_string(path).map_err(|e| Error::Os("read sysfs", e))?;
    let s = s.trim().trim_start_matches("0x");
    u16::from_str_radix(s, 16).map_err(|_| Error::Sysfs(format!("{}: not hex: {s:?}", path.display())))
}

/// IOMMU group number of the device at `bdf` (`/sys/bus/pci/devices/<bdf>/iommu_group`).
/// A device that exists but has no `iommu_group` link means the IOMMU is
/// off in firmware or on the kernel command line, and the error says so.
pub fn iommu_group_of(bdf: &str) -> Result<u32> {
    let link = device_dir(bdf)?.join("iommu_group");
    let target = fs::read_link(&link).map_err(|e| Error::Os("readlink iommu_group (is the IOMMU enabled?)", e))?;
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Sysfs(format!("{}: bad link", link.display())))?;
    name.parse()
        .map_err(|_| Error::Sysfs(format!("iommu_group name {name:?} is not a number")))
}

/// Name of the driver bound to `bdf`, if any (`None` also for a malformed
/// or absent address).
pub fn driver_of(bdf: &str) -> Option<String> {
    let link = device_dir(bdf).ok()?.join("driver");
    fs::read_link(link)
        .ok()
        .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// Vendor and device ID of `bdf` from sysfs.
pub fn ids_of(bdf: &str) -> Result<(u16, u16)> {
    let d = device_dir(bdf)?;
    Ok((read_hex_u16(&d.join("vendor"))?, read_hex_u16(&d.join("device"))?))
}

/// Check that `bdf` is a Xeon Phi 3120-series device.
pub fn require_phi(bdf: &str) -> Result<()> {
    let (v, d) = ids_of(bdf)?;
    if v != phi_regs::PCI_VENDOR_INTEL || d != phi_regs::PCI_DEVICE_3120 {
        return Err(Error::NotPhi(normalize_bdf(bdf)?, v, d));
    }
    Ok(())
}

/// Every Xeon Phi 3120-series device on the bus, by full BDF, lowest
/// address first (sysfs names are already canonical).
pub fn find_phis() -> Result<Vec<String>> {
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
    Ok(found)
}

/// The first Xeon Phi 3120-series device on the bus.
pub fn find_phi() -> Result<Option<String>> {
    Ok(find_phis()?.into_iter().next())
}

/// Resolve a BDF from an explicit argument, then `PHI_BDF`, then the card
/// index (`PHI_CARD`, else card 0 of `cards::list`). The result is
/// normalized; a malformed explicit or environment value is an error
/// rather than a silent fallback.
pub fn resolve_bdf(explicit: Option<&str>) -> Result<String> {
    if let Some(b) = explicit {
        return normalize_bdf(b);
    }
    if let Ok(b) = std::env::var("PHI_BDF") {
        if !b.trim().is_empty() {
            return normalize_bdf(&b);
        }
    }
    let index = crate::cards::index_from_env()?.unwrap_or(0);
    Ok(crate::cards::resolve(index)?.bdf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_pci_addresses() {
        assert_eq!(normalize_bdf("0000:2e:00.0").unwrap(), "0000:2e:00.0");
        assert_eq!(normalize_bdf("2e:00.0").unwrap(), "0000:2e:00.0");
        assert_eq!(normalize_bdf(" 0000:2E:00.0 ").unwrap(), "0000:2e:00.0");
        assert_eq!(normalize_bdf("0001:ff:1f.7").unwrap(), "0001:ff:1f.7");
        for bad in [
            "",
            "2e:00",
            "2e:00.8",
            "2e:20.0",
            "2e:0.0",
            "2e:00.0.0",
            "0000:2e:00",
            "00:2e:00.0",
            "zz:00.0",
            "0000:2e:00.x",
            ":2e:00.0",
        ] {
            assert!(matches!(normalize_bdf(bad), Err(Error::BadBdf(_))), "{bad:?} accepted");
        }
    }

    #[test]
    fn absent_device_is_a_clear_error() {
        // Domain 0xffff never exists on a real system.
        let e = iommu_group_of("ffff:ff:1f.7").unwrap_err();
        assert!(matches!(e, Error::Sysfs(ref s) if s.contains("no PCI device ffff:ff:1f.7")), "{e}");
        assert!(driver_of("ffff:ff:1f.7").is_none());
        assert!(driver_of("not a bdf").is_none());
        assert!(matches!(resolve_bdf(Some("nope")), Err(Error::BadBdf(_))));
    }
}
