# vpu_exec_regs.h

The byte sequences that store and load the card's whole vector register
file (zmm0..31 at offset 0, k0..7 at 2048, base register rax, clobbers
rcx), for the exec engine's entry and exit stubs (`vpu_exec.c`). They
are the card kernel's own (`asm/knc_vpu.h`, kernel patch 0024, generated
there by `knc-mvex-gen` from `host/crates/knc-mvex`), copied verbatim
with the `%%` of C inline asm reduced to `%` for a top-level `asm`
block. `vpu_exec.md` has the extraction command.
