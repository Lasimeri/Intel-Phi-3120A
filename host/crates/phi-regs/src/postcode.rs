//! POST codes written by the bootstrap and, later, by the running kernel.
//!
//! The register is [`crate::sbox::POSTCODE`], read as 32 bits. Its low two
//! bytes are **two ASCII characters**, low byte first: the code `"12"` is
//! stored as `0x3231`. The convention comes from Intel's POST code table
//! (MPSS 2.1 readme, `intc_dd_mic_2.1.6720-16`), which lists codes with
//! mixed case such as `"3d"` and `"dE"`, and was confirmed on this card on
//! 2026-09-13: the first read returned `0x6330`, the pair `"0c"`.

/// A POST code as read from the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Postcode(pub u32);

/// The bootstrap's "waiting for coprocessor OS download" state.
pub const POST_READY: &str = "12";

impl Postcode {
    /// The two code bytes in display order (low byte first).
    pub const fn bytes(self) -> [u8; 2] {
        [(self.0 & 0xff) as u8, ((self.0 >> 8) & 0xff) as u8]
    }

    /// The code as text when both bytes are printable ASCII, otherwise the
    /// raw register value in hex (which is how a garbage read shows up).
    pub fn text(self) -> String {
        let b = self.bytes();
        if b.iter().all(|c| c.is_ascii_graphic()) {
            String::from_utf8_lossy(&b).into_owned()
        } else {
            format!("{:#010x}", self.0)
        }
    }

    /// True when the bootstrap is waiting for an image.
    pub fn is_ready(self) -> bool {
        self.text().eq_ignore_ascii_case(POST_READY)
    }

    /// Human description from Intel's table (case-insensitive match).
    pub fn describe(self) -> Option<&'static str> {
        let t = self.text().to_ascii_lowercase();
        let d = match t.as_str() {
            "01" => "LIDT",
            "02" => "SBOX initialization",
            "03" => "Set GDDR top",
            "04" => "Begin memory test",
            "05" => "Program E820 table",
            "06" => "Initialize DBOX",
            "09" => "Enable caching",
            "0b" => "Pass initialization parameters to APs",
            "0c" => "Cache C code",
            "0d" => "Program MP table",
            "0e" => "Copy AP boot code to GDDR",
            "0f" => "Wake up APs",
            "10" => "Wait for APs to boot",
            "11" => "Signal host to download coprocessor OS",
            "12" => "Wait for coprocessor OS download (ready)",
            "13" => "Signal received from host to boot coprocessor OS",
            "15" => "Report platform information",
            "16" => "(undocumented; observed between GDDR finalize and enable caching, 2026-09-13)",
            "17" => "Page table setup",
            "30" => "Begin memory training",
            "31" => "Begin GDDR training to query memory modules",
            "32" => "Find GDDR training parameters in flash",
            "33" => "Begin GDDR MMIO training",
            "34" => "Begin GDDR RCOMP training",
            "35" => "Begin GDDR DCC disable training",
            "36" => "Begin GDDR HCK training",
            "37" => "Begin GDDR ucode training",
            "38" => "Begin GDDR vendor specific training",
            "39" => "Begin GDDR address training",
            "3a" => "Begin GDDR memory module identification",
            "3b" => "Begin GDDR WCK training",
            "3c" => "Begin GDDR read training with CDR enabled",
            "3d" => "Begin GDDR read training with CDR disabled",
            "3e" => "Begin GDDR write training",
            "3f" => "Finalize GDDR training",
            "40" => "Begin coprocessor OS authentication",
            "50" | "51" | "52" | "53" | "54" | "55" | "56" | "57" | "58" | "59" | "5a" | "5b" | "5c" | "5d" | "5e" | "5f" => {
                "Coprocessor OS loading and setup"
            }
            "75" => "int 10 Invalid TSS",
            "87" => "int 16 x87 FPU floating point error",
            "ac" => "int 17 Alignment check",
            "cc" => "int 18 Machine check",
            "db" => "int 1 Debug",
            "de" => "int 0 Divide error",
            "df" => "int 8 Double fault",
            "ee" => "Memory test failed",
            "f0" => "GDDR parameters not found in flash",
            "f1" => "GBOX PLL lock failure",
            "f2" => "GDDR failed memory training",
            "f3" => "GDDR memory module query failed",
            "f4" => "Memory preservation failure",
            "f5" => "int 12 Stack fault",
            "ff" => "Bootstrap finished execution",
            _ => return None,
        };
        Some(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_pair_convention() {
        assert_eq!(Postcode(0x3231).text(), "12");
        assert!(Postcode(0x3231).is_ready());
        assert!(Postcode(0xFFFF_3231).is_ready(), "upper bytes ignored");
        // Measured on this card 2026-09-13.
        assert_eq!(Postcode(0x6330).text(), "0c");
        assert_eq!(Postcode(0x6330).describe(), Some("Cache C code"));
        assert!(!Postcode(0x6330).is_ready());
        // Mixed case in Intel's table is matched case-insensitively.
        assert_eq!(
            Postcode(u32::from_le_bytes(*b"3D\0\0")).describe(),
            Some("Begin GDDR read training with CDR disabled")
        );
        // Non-printable bytes fall back to hex.
        assert_eq!(Postcode(0x0000_0012).text(), "0x00000012");
        assert!(Postcode(0x0000_0012).describe().is_none());
    }
}
