# lib.rs

`phi-vpu` is a library and a binary. The library is the two modules
both paths of the co-processor share: `proto` (the offsets, structures
and status codes of `card/vpu/vpu_proto.h` and `vpu_exec.h`, checked
against the C by `tools/vpu-layout-check.c`) and `window` (the shared
window). The binary (`main.rs`) is the explicit driver, `phi-vpu poly`;
`libphi512` is the other user.
