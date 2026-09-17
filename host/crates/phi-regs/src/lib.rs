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
//! - `micsboxdefine.h` / `micsboxstruct.h`: the KNC register list and bit
//!   layouts, `arch/x86/include/asm/mic/mic_knc/` in the same tree; MPSS
//!   3.8.6 ships a merged KNF/KNC copy as `include/mic/micsboxdefine.h`
//! - `micras_knc.c`: MPSS 3.8.6 `ras/micras_knc.c`, the sensor and PLL decodes
//! - `mic_dma_md.c` / `mic_dma_md.h`: MPSS 3.8.6 `dma/` and `include/mic/`, the DMA engine
//! - `SSDG`: Intel document 328207-002, with section number
//! - `ISA`: Intel document 327364-001, with appendix
//! - `boot.rst`: Linux `Documentation/arch/x86/boot.rst`
//!
//! Measurements on this card are cited by their record in `docs/results/`
//! or, for register reads, by the table in `docs/spec/sbox-registers.md`.

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
