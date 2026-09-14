//! The image-loading and boot sequence (`docs/research/boot-protocol.md`).

use std::time::Duration;

use phi_regs::bootparams::{self, parse_bzimage, BzImageInfo};
use phi_regs::memory;
use phi_regs::sbox;
use phi_ring::{ChannelKind, ChannelPlan, Region, VecMemory};

use crate::card::Card;
use crate::{Error, Result};

/// What to boot.
#[derive(Debug, Clone)]
pub struct BootImage {
    /// bzImage bytes.
    pub kernel: Vec<u8>,
    /// Optional initramfs bytes.
    pub initrd: Option<Vec<u8>>,
    /// Kernel command line, without the ring/`memmap` parameters, which are
    /// appended automatically unless `raw_cmdline` is set.
    pub cmdline: String,
    /// Card physical base of the ring region.
    pub ring_base: u64,
    /// Size of the ring region.
    pub ring_size: u64,
    /// If true, do not append the automatic parameters.
    pub raw_cmdline: bool,
}

/// Where everything ended up, for logging and for `docs/results/`.
#[derive(Debug, Clone)]
pub struct BootReport {
    /// Header facts of the kernel image.
    pub image: BzImageInfo,
    /// Download address from `SPAD2`.
    pub bootaddr: u64,
    /// Kernel image size.
    pub kernel_len: usize,
    /// Command line address (`bootaddr + kernel_len`).
    pub cmdline_addr: u64,
    /// The command line actually written.
    pub cmdline: String,
    /// Initramfs address and size, if any.
    pub initrd: Option<(u64, usize)>,
    /// BSP APIC ID the boot interrupt was sent to.
    pub apic_id: u32,
}

/// Default ring plan used by `phictl boot`.
pub const DEFAULT_CHANNELS: [ChannelPlan; 2] = [
    ChannelPlan {
        kind: ChannelKind::Console,
        h2c_size: 4096,
        c2h_size: 65536,
    },
    ChannelPlan {
        kind: ChannelKind::Network,
        h2c_size: 262_144,
        c2h_size: 262_144,
    },
];

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// The command line parameters that tell the kernel where the ring is and
/// how much memory it may use. `memmap=<size>$<base>` marks the ring region
/// reserved; `phi.ring=` is read by the KNC platform code and by `phinet`;
/// `mem=` caps the kernel at the card's GDDR, as Intel's loader did with the
/// aperture size, so that nothing the bootstrap lists above it (register
/// blocks, the SMPT window onto host memory) is ever treated as RAM.
pub fn ring_cmdline_params(base: u64, size: u64) -> String {
    format!(
        "memmap={}K${:#x} phi.ring={:#x},{:#x} mem={}M",
        size / 1024,
        base,
        base,
        size,
        memory::GDDR_BYTES_3120A >> 20
    )
}

/// Validate everything, format the ring region, copy the image, command
/// line and initramfs into card memory, patch the header, write `SPAD5`,
/// and send the boot interrupt. Returns after the interrupt is sent; the
/// caller watches the POST code and the console ring for progress.
pub fn boot(card: &Card, img: &BootImage) -> Result<BootReport> {
    // 1. Bootstrap must be waiting.
    card.wait_ready(Duration::from_secs(2))?;
    let dl = card.download_info();
    let bootaddr = dl.download_addr() as u64;
    if bootaddr == 0 || bootaddr > (1 << 31) {
        return Err(Error::Range(format!("implausible download address {bootaddr:#x} from SPAD2")));
    }

    // 2. Validate the image before touching the card.
    let info = parse_bzimage(&img.kernel)?;
    let kernel_len = img.kernel.len();

    // 3. Command line.
    let mut cmdline = img.cmdline.trim().to_string();
    if !img.raw_cmdline {
        if !cmdline.is_empty() {
            cmdline.push(' ');
        }
        cmdline.push_str(&ring_cmdline_params(img.ring_base, img.ring_size));
    }
    if cmdline.len() as u32 > info.cmdline_max {
        return Err(Error::Range(format!(
            "command line is {} bytes, kernel allows {}",
            cmdline.len(),
            info.cmdline_max
        )));
    }
    let cmdline_addr = bootaddr + kernel_len as u64;

    // 4. Initramfs placement: Intel put it at 2 * bootaddr. Check it clears
    //    the kernel image and command line.
    let initrd_addr = bootaddr * 2;
    let initrd = img.initrd.as_ref().map(|d| (initrd_addr, d.len()));
    if let Some((a, len)) = initrd {
        let end_of_cmdline = cmdline_addr + cmdline.len() as u64 + 1;
        if a < end_of_cmdline {
            return Err(Error::Range(format!(
                "initramfs at {a:#x} would overlap the kernel/cmdline ending at {end_of_cmdline:#x}"
            )));
        }
        if a + len as u64 > memory::GDDR_BYTES_3120A {
            return Err(Error::Range(format!("initramfs {a:#x}+{len:#x} exceeds card memory")));
        }
    }
    // The ring region must not collide with anything.
    let ring_end = img.ring_base + img.ring_size;
    if img.ring_base < bootaddr && ring_end > bootaddr {
        return Err(Error::Range("ring region overlaps the download address".into()));
    }
    if img.ring_base >= bootaddr && img.ring_base < initrd_addr + initrd.map_or(0, |(_, l)| l as u64) {
        return Err(Error::Range("ring region overlaps the kernel or initramfs".into()));
    }

    // 5. Format the ring region in host memory, then copy it in one go.
    let mut image = VecMemory::new(img.ring_size as usize);
    Region::format(&mut image, img.ring_size as usize, &DEFAULT_CHANNELS, now_ns())?;
    card.write_card_memory(img.ring_base, image.as_bytes())?;

    // 6. Kernel, with the header patched in the copy that goes to the card.
    let mut kernel = img.kernel.clone();
    if let Some((a, len)) = initrd {
        let (ab, sb) = bootparams::ramdisk_fields(a as u32, len as u32);
        kernel[bootparams::OFF_RAMDISK_IMAGE..bootparams::OFF_RAMDISK_IMAGE + 4].copy_from_slice(&ab);
        kernel[bootparams::OFF_RAMDISK_SIZE..bootparams::OFF_RAMDISK_SIZE + 4].copy_from_slice(&sb);
    }
    // Setting cmd_line_ptr as well covers both possible bootstrap behaviors
    // (see phi-regs bootparams.md); type_of_loader 0xFF = "undefined".
    kernel[bootparams::OFF_CMD_LINE_PTR..bootparams::OFF_CMD_LINE_PTR + 4].copy_from_slice(&(cmdline_addr as u32).to_le_bytes());
    kernel[bootparams::OFF_TYPE_OF_LOADER] = 0xFF;
    card.write_card_memory(bootaddr, &kernel)?;
    card.set_spad(sbox::SPAD_FW_SIZE, kernel_len as u32);

    // 7. Command line, NUL-terminated, right after the image.
    let mut cl = cmdline.clone().into_bytes();
    cl.push(0);
    card.write_card_memory(cmdline_addr, &cl)?;

    // 8. Initramfs.
    if let (Some(data), Some((a, _))) = (&img.initrd, initrd) {
        card.write_card_memory(a, data)?;
    }

    // 9. Go. No read-back: the first boot attempt reset the host, and a
    //    non-posted read through the aperture is one of the suspects; the
    //    boot interrupt is a register write that the card orders after the
    //    posted aperture writes. Use `phictl peek` to test aperture reads
    //    on their own.
    let apic_id = dl.apic_id();
    card.send_boot_interrupt();

    Ok(BootReport {
        image: info,
        bootaddr,
        kernel_len,
        cmdline_addr,
        cmdline,
        initrd,
        apic_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_params_are_what_the_kernel_expects() {
        let s = ring_cmdline_params(0x0200_0000, 1024 * 1024);
        assert_eq!(s, "memmap=1024K$0x2000000 phi.ring=0x2000000,0x100000 mem=6144M");
    }
}
