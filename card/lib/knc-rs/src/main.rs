//! `knc-demo`: Rust on the card, using the vector unit.
//!
//! Proves the binding end to end and measures it, so that the Rust number
//! sits next to the C and C++ ones from the same machine on the same day.
//! Every width round-trips against a scalar model written here, then the
//! decode rate is measured across threads. See main.md.
//!
//! ```text
//! card/lib/knc-rs/build.sh
//! phi put .../knc-demo /tmp/knc-demo && phi run /tmp/knc-demo 228
//! ```

use knc::{Block, Codec, Packed, BLOCK};
use std::env;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

const LANES: usize = 16;

fn low_mask(bits: u32) -> u32 {
    if bits >= 32 {
        u32::MAX
    } else {
        (1u32 << bits) - 1
    }
}

/// The layout written out, independent of the kernels: value `i` is lane
/// `i % 16` at position `i / 16`. Used to check the kernels rather than to
/// be fast.
fn scalar_unpack(out: &mut [i32], p: &[u32], bits: u32) {
    let m = low_mask(bits);

    for (i, slot) in out.iter_mut().enumerate().take(BLOCK) {
        let (lane, pos) = (i % LANES, i / LANES);
        let bit = pos * bits as usize;
        let (w, s) = (bit / 32, (bit % 32) as u32);
        let mut val = p[w * LANES + lane] >> s;

        if s + bits > 32 {
            val |= p[(w + 1) * LANES + lane] << (32 - s);
        }
        *slot = (val & m) as i32;
    }
}

fn check_every_width() -> usize {
    let mut values = Block::zeroed();
    let mut packed = Packed::zeroed();
    let mut out = Block::zeroed();
    let mut model = [0i32; BLOCK];
    let mut failed = 0;

    for bits in 1..=32u32 {
        let codec = Codec::new(bits).expect("1 to 32");
        let m = low_mask(bits);

        for (i, v) in values.as_mut_slice().iter_mut().enumerate() {
            *v = (0x9E37_79B9u32.wrapping_mul(i as u32 + 1) & m) as i32;
        }
        codec.pack(&mut packed, &values);
        codec.unpack(&mut out, &packed);
        scalar_unpack(&mut model, packed.as_slice(), bits);

        if out.as_slice() != values.as_slice() || model != *values.as_slice() {
            println!("  {bits:2} FAILED");
            failed += 1;
        }
    }
    println!("widths 1 to 32: {}", if failed == 0 { "all OK" } else { "FAILED" });

    // The one thing the C API cannot express, and the reason this crate
    // exists rather than a raw `extern "C"` block in every caller.
    assert_eq!(Codec::new(0).unwrap_err().0, 0);
    assert_eq!(Codec::new(33).unwrap_err().0, 33);

    failed
}

fn measure(threads: usize, blocks: usize, reps: usize, bits: u32) {
    let codec = Codec::new(bits).expect("1 to 32");
    let words = codec.words();

    // One packed block per thread, decoded `blocks` times into a buffer
    // large enough to leave cache behind.
    let mut seed = Block::zeroed();
    let m = low_mask(bits);
    for (i, v) in seed.as_mut_slice().iter_mut().enumerate() {
        *v = (0x9E37_79B9u32.wrapping_mul(i as u32 + 1) & m) as i32;
    }
    let mut source = Packed::zeroed();
    codec.pack(&mut source, &seed);
    let source = Arc::new(source);

    let start = Instant::now();
    let mut pool = Vec::with_capacity(threads);
    for _ in 0..threads {
        let source = Arc::clone(&source);
        pool.push(thread::spawn(move || {
            let mut out = vec![Block::zeroed(); blocks];
            let packed = vec![*source; blocks];

            for _ in 0..reps {
                for b in 0..blocks {
                    codec.unpack(&mut out[b], &packed[b]);
                }
            }
            // Keep the buffers alive and unoptimised.
            out[0][0] as u64 + packed[0][0] as u64
        }));
    }
    let mut sink = 0u64;
    for t in pool {
        sink = sink.wrapping_add(t.join().expect("worker"));
    }
    let secs = start.elapsed().as_secs_f64();
    let total = (threads * reps * blocks * BLOCK) as f64;

    println!(
        "{bits:5} {threads:8} {:12.1} {:10} words",
        total / secs / 1e6,
        words
    );
    let _ = sink;
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let threads: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let blocks: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(64);
    let reps: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(128);

    println!("knc-demo: Rust on the card, through libknc");
    let failed = check_every_width();

    println!("\n{:5} {:8} {:>12} {:>16}", "bits", "threads", "unpack M/s", "packed");
    for bits in [1u32, 8, 11, 16, 32] {
        measure(threads, blocks, reps, bits);
    }

    if failed != 0 {
        std::process::exit(1);
    }
}
