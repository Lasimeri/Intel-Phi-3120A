# phi-regs / Cargo.toml

Library crate with no dependencies and `#![forbid(unsafe_code)]`: the
register map, decoders, POST codes, boot header offsets and card memory
map for the Xeon Phi 3120A. Version, edition, license and repository come
from the workspace `host/Cargo.toml`.

Every other host crate depends on it (`phi-vfio` for the PCI IDs,
`phi-hw`, `phictl`, `phitop` for everything else), which is why it stays
dependency-free: it must compile and test on a machine without the card,
without `libc`, in a second.
