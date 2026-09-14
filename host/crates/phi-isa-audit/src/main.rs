//! `phi-isa-audit`: report instructions Knights Corner cannot execute.
//!
//! Exit status: 0 if no illegal instructions (suspects allowed with
//! `--allow-suspect`, otherwise they also fail), 1 if any were found,
//! 2 on usage or file errors. Reasons named with `--ignore` are reported
//! but excluded from the verdict; the calling script documents why.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use phi_isa_audit::{audit_elf, Hit, Severity};

/// Report instructions in an x86-64 ELF that Knights Corner cannot execute.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// ELF file (executable, shared object, relocatable, or vmlinux).
    file: PathBuf,
    /// List every hit instead of one example per reason.
    #[arg(long)]
    list: bool,
    /// With --list, stop after this many hits.
    #[arg(long, default_value_t = 200)]
    max: usize,
    /// Do not fail on suspect (undocumented) instructions such as multi-byte NOPs.
    #[arg(long)]
    allow_suspect: bool,
    /// Reason (as printed in the summary, e.g. XSAVE) whose hits are reported
    /// but do not count; repeatable. For documented false positives only.
    #[arg(long = "ignore", value_name = "REASON")]
    ignore: Vec<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let data = match std::fs::read(&args.file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("phi-isa-audit: {}: {e}", args.file.display());
            return ExitCode::from(2);
        }
    };
    let mut report = match audit_elf(&data) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("phi-isa-audit: {}: {e}", args.file.display());
            return ExitCode::from(2);
        }
    };
    // Hits the caller declared documented false positives (for example
    // `--ignore XSAVE` for std_detect's `xgetbv` behind a CPUID.OSXSAVE
    // check) are split off: printed, never counted.
    let mut ignored: Vec<Hit> = Vec::new();
    if !args.ignore.is_empty() {
        let (ign, keep): (Vec<Hit>, Vec<Hit>) = report
            .hits
            .into_iter()
            .partition(|h| args.ignore.iter().any(|r| r.eq_ignore_ascii_case(&h.reason)));
        ignored = ign;
        report.hits = keep;
    }
    println!(
        "{}: {} executable section(s) [{}], {} instructions",
        args.file.display(),
        report.sections.len(),
        if report.sections.len() <= 8 {
            report.sections.join(", ")
        } else {
            format!("{} ... {}", report.sections[0], report.sections[report.sections.len() - 1])
        },
        report.instructions
    );
    if args.list {
        for h in report.hits.iter().take(args.max) {
            let sym = h.symbol.as_ref().map(|(n, o)| format!(" <{n}+{o:#x}>")).unwrap_or_default();
            println!(
                "{:<8} {:<24} {:#012x}{sym}: {}  [{}]",
                format!("{:?}", h.severity).to_uppercase(),
                h.reason,
                h.address,
                h.text,
                h.section
            );
        }
        if report.hits.len() > args.max {
            println!("... {} more (raise --max)", report.hits.len() - args.max);
        }
    } else {
        println!("{:<8} {:>8}  {:<24} example", "severity", "count", "reason");
        for ((sev, reason), (count, ex)) in report.by_reason() {
            let sym = ex.symbol.as_ref().map(|(n, o)| format!(" <{n}+{o:#x}>")).unwrap_or_default();
            println!(
                "{:<8} {:>8}  {:<24} {:#012x}{sym}: {}",
                format!("{sev:?}").to_uppercase(),
                count,
                reason,
                ex.address,
                ex.text
            );
        }
    }
    for h in &ignored {
        let sym = h.symbol.as_ref().map(|(n, o)| format!(" <{n}+{o:#x}>")).unwrap_or_default();
        println!(
            "{:<8} {:<24} {:#012x}{sym}: {}  [{}]",
            "IGNORED", h.reason, h.address, h.text, h.section
        );
    }
    let illegal = report.count(Severity::Illegal);
    let suspect = report.count(Severity::Suspect);
    if ignored.is_empty() {
        println!("result: {illegal} illegal, {suspect} suspect");
    } else {
        println!("result: {illegal} illegal, {suspect} suspect, {} ignored", ignored.len());
    }
    if illegal > 0 || (suspect > 0 && !args.allow_suspect) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    /// Any distribution binary is built for x86-64 baseline and contains
    /// CMOV and SSE; the audit must see them. Skipped where /usr/bin/ls is
    /// not an ELF (non-Linux CI).
    #[test]
    fn distro_binary_is_full_of_illegal_instructions() {
        let Ok(data) = std::fs::read("/usr/bin/ls") else { return };
        if !data.starts_with(b"\x7FELF") {
            return;
        }
        let r = phi_isa_audit::audit_elf(&data).unwrap();
        assert!(r.count(phi_isa_audit::Severity::Illegal) > 0);
        assert!(r.hits.iter().any(|h| h.reason == "CMOV"));
    }
}
