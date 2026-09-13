//! Hardware constants for the Intel Xeon Phi 3120A (Knights Corner, x100).
//!
//! This crate performs no I/O. It is the single place where register
//! offsets, bit layouts, POST codes, the Linux boot header layout, and the
//! card memory map are written down, each with the document or source file
//! it was taken from. Everything else in the workspace imports from here so
//! that a wrong constant can only be wrong in one place.
//!
//! Sources are abbreviated in doc comments as:
//! - `mic_x100.h` / `mic_x100.c`: mainline Linux v5.9, `drivers/misc/mic/host/`
//! - `intelmic.c`: Intel's k1om kernel `arch/x86/kernel/intelmic.c` (2.6.38.8+mpss3.5.1)
//! - `SSDG`: Intel document 328207-002, with section number
//! - `ISA`: Intel document 327364-001, with appendix
//! - `boot.rst`: Linux `Documentation/arch/x86/boot.rst`

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod bootparams;
pub mod memory;
pub mod postcode;
pub mod sbox;

/// PCI vendor ID of the card (Intel).
pub const PCI_VENDOR_INTEL: u16 = 0x8086;
/// PCI device ID of the "Xeon Phi coprocessor 3120 series" (`mic_x100.c` device table).
pub const PCI_DEVICE_3120: u16 = 0x225d;
/// PCI subsystem device ID observed on this card; linux-hardware.org decodes it as 3120A/3140A.
pub const PCI_SUBSYSTEM_3120A: u16 = 0x3c98;
