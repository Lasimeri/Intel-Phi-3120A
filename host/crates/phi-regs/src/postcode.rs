//! POST codes written by the bootstrap and, later, by the running kernel.
//!
//! The register is [`crate::sbox::POSTCODE`], a 32-bit read whose low byte
//! is the code. Known bootstrap values are from the Intel MPSS 2.1 readme
//! (`intc_dd_mic_2.1.6720-16`); the complete table ships in MPSS
//! `mpss-boot-files` documentation (see `vendor/`).

/// A POST code as read from the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Postcode(pub u32);

/// The bootstrap's "waiting for coprocessor OS download" state. The card
/// sits here after power-on or reset until the host sends the boot
/// interrupt.
pub const POST_READY: u8 = 0x12;

impl Postcode {
    /// Low byte, which is the code proper.
    pub const fn code(self) -> u8 {
        (self.0 & 0xff) as u8
    }

    /// True when the bootstrap is waiting for an image.
    pub const fn is_ready(self) -> bool {
        self.code() == POST_READY
    }

    /// Human description for known bootstrap codes.
    pub const fn describe(self) -> Option<&'static str> {
        Some(match self.code() {
            0x01 => "bootstrap: LIDT",
            0x02 => "bootstrap: SBOX initialization",
            0x03 => "bootstrap: set GDDR top",
            0x05 => "bootstrap: program E820 table",
            0x06 => "bootstrap: initialize DBOX",
            0x0E => "bootstrap: copy AP boot code to GDDR",
            0x12 => "bootstrap: waiting for coprocessor OS download (ready)",
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_code_is_recognized() {
        assert!(Postcode(0x12).is_ready());
        assert!(Postcode(0xFFFF_FF12).is_ready(), "only the low byte matters");
        assert!(!Postcode(0x0E).is_ready());
        assert!(Postcode(0x0E).describe().is_some());
        assert!(Postcode(0x7F).describe().is_none());
    }
}
