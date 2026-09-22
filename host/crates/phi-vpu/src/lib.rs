//! Host side of the AVX-512 co-processor: the protocol the card worker
//! speaks (`proto`) and the shared window it is spoken through (`window`).
//! The `phi-vpu` binary drives the explicit path; `libphi512` uses the
//! same two modules for the seamless one. See lib.md.
pub mod proto;
pub mod window;
