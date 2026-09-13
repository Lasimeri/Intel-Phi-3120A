//! Rust half of the phase P2 exit test: a `staticlib` linked into
//! `hello.c`. Exercises the knc64-x87 ABI from the Rust side: `f64`
//! arguments and return across `extern "C"`, `f64` arithmetic compiled by
//! the patched backend (x87, no SSE), a 64-bit `select` (no CMOV), and
//! `std` built for the custom target (`String`, `Vec`, formatting).

/// Called from C with two doubles; must return `5.0` for `(3.0, 4.0)`.
///
/// # Safety
/// Plain by-value arguments; nothing to uphold.
#[no_mangle]
pub extern "C" fn rust_hypot(a: f64, b: f64) -> f64 {
    let s = describe(a, b);
    // Keep the formatting path alive so a broken std shows up as a crash.
    std::hint::black_box(&s);
    (a * a + b * b).sqrt()
}

/// A 64-bit select and some allocation, so the audit sees `std` code paths.
fn describe(a: f64, b: f64) -> String {
    let larger: i64 = if a > b { a as i64 } else { b as i64 };
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("{a}"));
    parts.push(format!("{b}"));
    format!("larger={larger} parts={}", parts.join(","))
}

#[cfg(test)]
mod tests {
    #[test]
    fn hypot_is_five() {
        assert_eq!(super::rust_hypot(3.0, 4.0), 5.0);
    }
}
