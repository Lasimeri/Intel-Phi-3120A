//! Audit engine: decode every executable section of an x86-64 ELF and
//! classify each instruction against the Knights Corner deletion list
//! (`docs/research/isa-deletions.md`).
//!
//! Classification uses iced-x86's per-instruction CPUID feature list,
//! which is exactly the question at hand ("which CPUID feature bit gates
//! this instruction?"), plus a few mnemonic checks for instructions that
//! belong to the base ISA on paper but are deleted on KNC (`IN`/`OUT`,
//! `SYSENTER`/`SYSEXIT`).

#![warn(missing_docs)]

use std::collections::BTreeMap;

use iced_x86::{CpuidFeature, Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter, Mnemonic};
use object::{Architecture, Object, ObjectSection, ObjectSymbol, SectionKind};

/// How bad a hit is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Documented as unsupported (ISA reference Appendix B, SSDG 4.2, or
    /// absent from the supported table with corroboration).
    Illegal,
    /// Not documented either way; must be measured on the card
    /// (multi-byte NOP encodings).
    Suspect,
}

/// One flagged instruction.
#[derive(Clone, Debug)]
pub struct Hit {
    /// Severity.
    pub severity: Severity,
    /// Why: a CPUID feature name (`CMOV`, `SSE2`) or a mnemonic-based rule (`IN/OUT`).
    pub reason: String,
    /// Address of the instruction.
    pub address: u64,
    /// Intel-syntax disassembly.
    pub text: String,
    /// Enclosing symbol and offset, if known.
    pub symbol: Option<(String, u64)>,
    /// Section name.
    pub section: String,
}

/// Summary of one file.
#[derive(Debug, Default)]
pub struct Report {
    /// Executable sections scanned.
    pub sections: Vec<String>,
    /// Total instructions decoded.
    pub instructions: u64,
    /// Knights Corner vector instructions (MVEX prefix), counted among `instructions`.
    pub vector: u64,
    /// All hits, in address order.
    pub hits: Vec<Hit>,
}

impl Report {
    /// Number of hits at a severity.
    pub fn count(&self, s: Severity) -> usize {
        self.hits.iter().filter(|h| h.severity == s).count()
    }

    /// Hits grouped by reason with counts and one example each.
    pub fn by_reason(&self) -> BTreeMap<(Severity, String), (usize, Hit)> {
        let mut m: BTreeMap<(Severity, String), (usize, Hit)> = BTreeMap::new();
        for h in &self.hits {
            let e = m.entry((h.severity, h.reason.clone())).or_insert_with(|| (0, h.clone()));
            e.0 += 1;
        }
        m
    }
}

/// Classify one CPUID feature. `None` means allowed on KNC.
///
/// Matching is on the feature's name so the list reads like the ISA
/// document and does not depend on iced-x86's enum spelling.
pub fn classify_feature(f: CpuidFeature) -> Option<Severity> {
    let name = format!("{f:?}");
    let illegal = [
        "CMOV",
        "MMX",
        "SSE",
        "SSE2",
        "SSE3",
        "SSSE3",
        "SSE4_1",
        "SSE4_2",
        "SSE4A",
        "AVX",
        "AVX2",
        "FMA",
        "F16C",
        "CMPXCHG16B",
        "MONITOR",
        "MWAITX",
        "PAUSE",
        "PREFETCHW",
        "PREFETCHWT1",
        "CLFSH",
        "CLFLUSHOPT",
        "CLWB",
        "RDTSCP",
        "MOVBE",
        "POPCNT",
        "LZCNT",
        "BMI1",
        "BMI2",
        "XSAVE",
        "XSAVEOPT",
        "XSAVEC",
        "XSAVES",
        "AES",
        "PCLMULQDQ",
        "SHA",
        "RDRAND",
        "RDSEED",
        "ADX",
        "FSGSBASE",
        "INVPCID",
        "RDPID",
        "SMAP",
        "CET_IBT",
        "CET_SS",
        "MPX",
        "3DNOW",
        "3DNOWEXT",
        "TBM",
        "XOP",
        "FMA4",
        "LWP",
        "ENQCMD",
        "SERIALIZE",
        "TSXLDTRK",
        "WAITPKG",
        "MOVDIRI",
        "MOVDIR64B",
        "PTWRITE",
        "UINTR",
        "AMX_TILE",
        "AMX_INT8",
        "AMX_BF16",
        "KL",
        "AESKLE",
        "HRESET",
        "LAHF_SAHF_64",
        "RTM",
        "HLE",
        "PKU",
        "OSPKE",
        "CLDEMOTE",
        "GFNI",
        "VAES",
        "VPCLMULQDQ",
    ];
    if name.starts_with("AVX512") || name.starts_with("AVX_VNNI") || name.starts_with("AMX") {
        return Some(Severity::Illegal);
    }
    if illegal.contains(&name.as_str()) {
        return Some(Severity::Illegal);
    }
    if name == "MULTIBYTENOP" || name == "CET_IBT" {
        // 0F 1F NOP forms and endbr64 (F3 0F 1E FA) are NOPs on CPUs without
        // the feature; whether the P54C-derived decoder accepts the 0F 1x
        // opcodes is undocumented and must be measured.
        return Some(Severity::Suspect);
    }
    None
}

/// Instructions iced-x86 files under a feature bit but which Knights Corner
/// documents as supported: the MXCSR load/store pair (ISA reference App.
/// B.3 LDMXCSR, B.6 STMXCSR) touches no XMM register, and the vector mask
/// register instructions (`kand`, `kandn`, `knot`, `kor`, `kxnor`, `kxor`,
/// `kmov`, `kortest`; ISA reference chapter 6) use the VEX.128.0F.W0
/// encodings that AVX-512 later took for its 16-bit mask forms, so the
/// decoder names them `k...w` and files them under AVX512F.
pub fn allowed_despite_feature(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(
        m,
        Ldmxcsr | Stmxcsr | Kandw | Kandnw | Knotw | Korw | Kxnorw | Kxorw | Kmovw | Kortestw
    )
}

/// Length of a Knights Corner MVEX-encoded instruction starting at `b`, or
/// `None` if `b` does not start one. In 64-bit mode 62H is always a
/// four-byte vector prefix; bit 2 of its third byte is 0 for MVEX (ISA
/// reference 327364-001, section 3.3, and Intel's k1om kernel macros) and
/// 1 for EVEX, which iced-x86 would decode as AVX-512. The length is the
/// prefix, the opcode, ModRM, an optional SIB and displacement as in every
/// x86 instruction, and an immediate byte where the opcode takes one. In
/// the 0F map that is `C2` (the compares), `70` (`vpshufd`) and `72` (the
/// immediate-count shifts, `/6` `/2` `/4`); the whole 0F3A map takes one;
/// no 0F38 opcode does. The disp8*N compression changes no lengths.
pub fn mvex_length(b: &[u8]) -> Option<usize> {
    if b.len() < 6 || b[0] != 0x62 || b[2] & 0x04 != 0 {
        return None;
    }
    let map = b[1] & 0x03;
    if map == 0 {
        return None;
    }
    let opcode = b[4];
    let modrm = b[5];
    let (m, rm) = (modrm >> 6, modrm & 7);
    let mut len = 6;
    if m != 3 {
        if rm == 4 {
            len += 1; // SIB
        }
        len += match m {
            0 if rm == 5 => 4,
            0 => 0,
            1 => 1,
            _ => 4,
        };
    }
    if map == 3 || (map == 1 && matches!(opcode, 0x70 | 0x72 | 0xc2)) {
        len += 1;
    }
    (b.len() >= len).then_some(len)
}

/// Mnemonic rules for base-ISA instructions KNC deletes (ISA App. B.2).
pub fn classify_mnemonic(m: Mnemonic) -> Option<(Severity, &'static str)> {
    use Mnemonic::*;
    match m {
        In | Insb | Insw | Insd => Some((Severity::Illegal, "IN/INS (port I/O)")),
        Out | Outsb | Outsw | Outsd => Some((Severity::Illegal, "OUT/OUTS (port I/O)")),
        Sysenter | Sysexit | Sysexitq => Some((Severity::Illegal, "SYSENTER/SYSEXIT")),
        Fcmovb | Fcmove | Fcmovbe | Fcmovu | Fcmovnb | Fcmovne | Fcmovnbe | Fcmovnu => Some((Severity::Illegal, "FCMOVcc")),
        Fcomi | Fcomip | Fucomi | Fucomip => Some((Severity::Illegal, "FCOMI/FUCOMI")),
        _ => None,
    }
}

/// Decode `code` at `base` and push hits into `out`. Returns instruction count.
pub fn scan_bytes(code: &[u8], base: u64, section: &str, symbols: &[(u64, u64, String)], out: &mut Vec<Hit>) -> u64 {
    let (n, _) = scan_bytes_counting(code, base, section, symbols, out);
    n
}

/// As `scan_bytes`, also returning how many of the instructions were
/// Knights Corner vector instructions.
pub fn scan_bytes_counting(code: &[u8], base: u64, section: &str, symbols: &[(u64, u64, String)], out: &mut Vec<Hit>) -> (u64, u64) {
    let mut decoder = Decoder::with_ip(64, code, base, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let mut instr = Instruction::default();
    let mut text = String::new();
    let mut count = 0u64;
    let mut vector = 0u64;
    while decoder.can_decode() {
        let pos = decoder.position();
        if let Some(len) = mvex_length(&code[pos..]) {
            // A KNC vector instruction: legal by definition, opaque to iced-x86.
            count += 1;
            vector += 1;
            decoder.set_position(pos + len).expect("within the buffer");
            decoder.set_ip(base + (pos + len) as u64);
            continue;
        }
        decoder.decode_out(&mut instr);
        count += 1;
        if instr.is_invalid() {
            continue;
        }
        let mut worst: Option<(Severity, String)> = None;
        if allowed_despite_feature(instr.mnemonic()) {
            continue;
        }
        for &f in instr.cpuid_features() {
            if let Some(sev) = classify_feature(f) {
                let name = format!("{f:?}");
                match &worst {
                    Some((w, _)) if *w <= sev => {}
                    _ => worst = Some((sev, name)),
                }
            }
        }
        if let Some((sev, why)) = classify_mnemonic(instr.mnemonic()) {
            match &worst {
                Some((w, _)) if *w <= sev => {}
                _ => worst = Some((sev, why.to_string())),
            }
        }
        if let Some((severity, reason)) = worst {
            text.clear();
            formatter.format(&instr, &mut text);
            let symbol = symbols
                .iter()
                .find(|(a, s, _)| instr.ip() >= *a && (instr.ip() < a + s || (*s == 0 && instr.ip() == *a)))
                .map(|(a, _, n)| (n.clone(), instr.ip() - a));
            out.push(Hit {
                severity,
                reason,
                address: instr.ip(),
                text: text.clone(),
                symbol,
                section: section.to_string(),
            });
        }
    }
    (count, vector)
}

/// Audit a file: an ELF object, executable, or shared object, or an `ar`
/// archive (`libc.a`, `libclang_rt.builtins.a`) whose ELF members are
/// audited one by one with the member name prefixed to each section.
pub fn audit_elf(data: &[u8]) -> anyhow::Result<Report> {
    if data.starts_with(b"!<arch>\n") {
        return audit_archive(data);
    }
    audit_one(data, "")
}

fn audit_archive(data: &[u8]) -> anyhow::Result<Report> {
    use object::read::archive::ArchiveFile;
    let archive = ArchiveFile::parse(data)?;
    let mut report = Report::default();
    for member in archive.members() {
        let member = member?;
        let name = String::from_utf8_lossy(member.name()).into_owned();
        let bytes = member.data(data)?;
        if !bytes.starts_with(b"\x7FELF") {
            continue; // symbol table, string table
        }
        let r = audit_one(bytes, &format!("{name}:"))?;
        report.instructions += r.instructions;
        report.vector += r.vector;
        report.sections.extend(r.sections);
        report.hits.extend(r.hits);
    }
    Ok(report)
}

fn audit_one(data: &[u8], prefix: &str) -> anyhow::Result<Report> {
    let file = object::File::parse(data)?;
    if file.architecture() != Architecture::X86_64 {
        anyhow::bail!("not an x86-64 ELF (architecture {:?})", file.architecture());
    }
    let mut symbols: Vec<(u64, u64, String)> = file
        .symbols()
        .chain(file.dynamic_symbols())
        .filter(|s| s.address() != 0)
        .filter_map(|s| s.name().ok().map(|n| (s.address(), s.size(), n.to_string())))
        .collect();
    // Prefer sized symbols; sort by address then by size descending so the
    // linear search finds the enclosing function first.
    symbols.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    symbols.dedup_by(|a, b| a.0 == b.0 && a.2 == b.2);

    let mut report = Report::default();
    for section in file.sections() {
        if section.kind() != SectionKind::Text {
            continue;
        }
        // Kernel alternatives: replacement slots are only copied into the
        // text when the CPU feature they are keyed on is present, so their
        // bytes never execute on a CPU that lacks the feature.
        if section.name().ok() == Some(".altinstr_replacement") {
            continue;
        }
        let name = format!("{prefix}{}", section.name().unwrap_or("?"));
        let data = match section.data() {
            Ok(d) if !d.is_empty() => d,
            _ => continue,
        };
        let (n, v) = scan_bytes_counting(data, section.address(), &name, &symbols, &mut report.hits);
        report.instructions += n;
        report.vector += v;
        report.sections.push(name);
    }
    report.hits.sort_by_key(|h| h.address);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_the_known_offenders_and_passes_clean_code() {
        // cmove eax,ecx ; nop ; pause ; mfence ; nopw [rax+rax] ; prefetcht0 [rax] ; cmpxchg16b [rax] ; in al,dx ; ret
        let code: Vec<u8> = vec![
            0x0F, 0x44, 0xC1, // cmove
            0x90, // nop
            0xF3, 0x90, // pause
            0x0F, 0xAE, 0xF0, // mfence
            0x66, 0x0F, 0x1F, 0x44, 0x00, 0x00, // nopw
            0x0F, 0x18, 0x08, // prefetcht0
            0x48, 0x0F, 0xC7, 0x08, // cmpxchg16b
            0xEC, // in al,dx
            0xC3, // ret
        ];
        let mut hits = Vec::new();
        let n = scan_bytes(&code, 0x1000, ".text", &[(0x1000, 32, "f".into())], &mut hits);
        assert_eq!(n, 9);
        let reasons: Vec<&str> = hits.iter().map(|h| h.reason.as_str()).collect();
        assert!(reasons.contains(&"CMOV"), "{reasons:?}");
        assert!(reasons.contains(&"PAUSE"), "{reasons:?}");
        assert!(reasons.iter().any(|r| r.starts_with("SSE")), "fence and prefetch: {reasons:?}");
        assert!(reasons.iter().any(|r| r.contains("CMPXCHG16B")), "{reasons:?}");
        assert!(reasons.iter().any(|r| r.starts_with("IN/INS")), "{reasons:?}");
        let suspects: Vec<_> = hits.iter().filter(|h| h.severity == Severity::Suspect).collect();
        assert_eq!(suspects.len(), 1, "the nopw is suspect, not illegal");
        assert_eq!(hits[0].symbol.as_ref().map(|(n, o)| (n.as_str(), *o)), Some(("f", 0)));
        // Plain nop and ret produce nothing.
        let mut clean = Vec::new();
        scan_bytes(&[0x90, 0xC3], 0, ".text", &[], &mut clean);
        assert!(clean.is_empty());
    }
}

#[cfg(test)]
mod knc_vector_tests {
    use super::*;

    #[test]
    fn mvex_instructions_are_legal_and_sized() {
        // vmovaps [rax+64], zmm1 (Intel k1om macro form) ; kmovw ecx,k1 ; vcmppd k1{k1},zmm10,zmm8,1 ; ret
        let code: Vec<u8> = vec![
            0x62, 0xf1, 0x78, 0x08, 0x29, 0x88, 0x40, 0x00, 0x00, 0x00, // vmovaps store, disp32
            0xc5, 0xf8, 0x93, 0xc9, // kmovw ecx, k1
            0x62, 0xd1, 0xa9, 0x09, 0xc2, 0xc8, 0x01, // vcmppd with imm8
            0x62, 0xf1, 0xf9, 0x08, 0x58, 0xd1, // vaddpd zmm2{k1}, zmm0, zmm1
            0xc3, // ret
        ];
        assert_eq!(mvex_length(&code), Some(10));
        assert_eq!(mvex_length(&code[14..]), Some(7));
        assert_eq!(mvex_length(&code[21..]), Some(6));
        // The immediate-count shifts are opcode 72 in the 0F map with an
        // opcode extension in ModRM.reg and an imm8; without the imm8 the
        // decoder loses sync and reports rubbish for the rest of the
        // section. Bytes from card/examples/vpu_int.S, run on the card
        // (docs/results/2026-09-20-mvex-integer.md).
        assert_eq!(
            mvex_length(&[0x62, 0xf1, 0x69, 0x08, 0x72, 0xd0, 0x0b, 0x90]),
            Some(7),
            "vpsrld zmm2, zmm0, 11"
        );
        assert_eq!(
            mvex_length(&[0x62, 0xf1, 0x69, 0x08, 0x72, 0xb7, 0x00, 0x00, 0x00, 0x00, 0x0b, 0x90]),
            Some(11),
            "vpslld zmm2, [rdi+0], 11: disp32 and imm8"
        );
        assert_eq!(
            mvex_length(&[0x62, 0xf1, 0x7d, 0x48, 0x28, 0x07]),
            None,
            "EVEX (bit 2 set) is not MVEX"
        );
        let mut hits = Vec::new();
        let (n, v) = scan_bytes_counting(&code, 0, ".text", &[], &mut hits);
        assert_eq!((n, v), (5, 3));
        assert!(hits.is_empty(), "{hits:?}");
    }
}
