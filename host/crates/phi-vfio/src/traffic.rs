//! Byte counters for the PCIe traffic this process drives: every bulk
//! aperture copy (`Mapping::read_bytes` and `write_bytes`) and, from
//! `phi-hw`, every DMA copy adds to one of four counters, so the daemon
//! can report what it has moved in each direction. Single 32-bit register
//! accesses (ring indices, SBOX registers) are not counted: they are
//! polling, not payload. See traffic.md.

use std::sync::atomic::{AtomicU64, Ordering};

/// Bytes the DMA engine copied into card memory.
pub static DMA_TO_CARD: AtomicU64 = AtomicU64::new(0);
/// Bytes the DMA engine copied into host memory.
pub static DMA_FROM_CARD: AtomicU64 = AtomicU64::new(0);
/// DMA copies (descriptors) into card memory; with the bytes, the mean
/// copy size, which is what the block services are bound by.
pub static DMA_COPIES_TO_CARD: AtomicU64 = AtomicU64::new(0);
/// DMA copies into host memory.
pub static DMA_COPIES_FROM_CARD: AtomicU64 = AtomicU64::new(0);
/// Bytes written through the aperture (BAR0) by the host CPU.
pub static APERTURE_TO_CARD: AtomicU64 = AtomicU64::new(0);
/// Bytes read through the aperture by the host CPU.
pub static APERTURE_FROM_CARD: AtomicU64 = AtomicU64::new(0);

/// Add `bytes` to a counter.
pub fn add(counter: &AtomicU64, bytes: usize) {
    counter.fetch_add(bytes as u64, Ordering::Relaxed);
}

/// The four counters: DMA to the card, DMA from it, aperture to it, aperture from it.
pub fn snapshot() -> [u64; 4] {
    [
        DMA_TO_CARD.load(Ordering::Relaxed),
        DMA_FROM_CARD.load(Ordering::Relaxed),
        APERTURE_TO_CARD.load(Ordering::Relaxed),
        APERTURE_FROM_CARD.load(Ordering::Relaxed),
    ]
}

/// The two copy counts: DMA copies to the card, DMA copies from it.
pub fn copies() -> [u64; 2] {
    [
        DMA_COPIES_TO_CARD.load(Ordering::Relaxed),
        DMA_COPIES_FROM_CARD.load(Ordering::Relaxed),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_accumulate() {
        let before = snapshot();
        add(&APERTURE_TO_CARD, 10);
        add(&DMA_FROM_CARD, 5);
        let after = snapshot();
        assert!(after[2] >= before[2] + 10);
        assert!(after[1] >= before[1] + 5);
    }
}
