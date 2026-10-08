# defs.inc: what every source of phictl includes

- **The GNU-stack note**, so `ld` marks the stack non-executable.
- **System call numbers**, x86-64, from
  `arch/x86/entry/syscalls/syscall_64.tbl`.
  `ppoll` (271) is the daemon's idle wait; `prctl` (157) with
  `PR_SET_TIMERSLACK` (29, `linux/prctl.h`) sets its timer slack.
- **Flags** from the uapi headers named beside each group: `open`, `fcntl`,
  `mmap`, `madvise`, `poll`, sockets, signals, clocks, the errno values
  the code tests.
- **VFIO**: the ioctl numbers are `_IO(';', 100 + n)`, which encodes as
  `0x3b00 | (100 + n)` (no direction, no size: VFIO carries sizes in the
  structs' `argsz`). Structure sizes used: `vfio_group_status` 8,
  `vfio_device_info` 24, `vfio_region_info` 32 (`size` at 16, `offset` at
  24), `vfio_iommu_type1_dma_map` 32 (`vaddr` 8, `iova` 16, `size` 24);
  the same values `host/crates/phi-vfio/src/ioctl.rs` pins by test.
- **The card**: BAR 0 is the aperture (card memory), BAR 4 the registers;
  SBOX offsets relative to `SBOX_BASE` (0x10000) inside BAR 4; `POSTCODE`
  is a raw BAR 4 offset (0x242c, DBOX side). Every offset comes from
  `host/crates/phi-regs/src/sbox.rs`, which names its Intel source per
  register (`mic_x100.h`, the k1om `micsboxdefine.h`, `mic_dma_md.h`).
- **Card memory**: 6 GiB of GDDR, the SMPT window at 0x80_0000_0000 with
  16 GiB pages, the ring region at 256 MiB, 16 MiB
  (`host/crates/phi-regs/src/memory.rs`).
- **The bzImage header** offsets (`Documentation/arch/x86/boot.rst`).
- **The ring region layout** (`docs/spec/ring-protocol.md`): header
  fields, channel descriptor fields, ring header fields, the channel
  kinds, the magic values.
- **The rpc wire** (`host/crates/phi-rpc/src/lib.rs`): frame limit,
  string and list limits, the message tags 1 to 21; a decoder's
  layout (`DEC_*`) and buffer size.
- **`SYS nr`**: the system call with its arguments already in `rdi`,
  `rsi`, `rdx`, `r10`, `r8`, `r9`; `rax` is the result or `-errno`;
  `rcx` and `r11` are clobbered by the instruction.
- **`STR name, text`**: a NUL-terminated string in `.rodata`, referenced
  as `name(%rip)`, usable in the middle of a function.
