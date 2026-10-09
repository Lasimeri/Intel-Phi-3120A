# defs.inc: phitop's records

Includes `../phictl/defs.inc` (system calls, the wire, the decoder
layout) and adds: the terminal ioctls and termios fields
(`asm-generic/ioctls.h`, `asm-generic/termbits.h`), the signal numbers,
and the record layouts of `model.S` and `main.S`:

- a device entry (`DEV_*`): name length, name (up to 59 bytes), bytes
  read, bytes written; 80 bytes;
- a process entry (`PR_*`): pid, state byte, kernel-thread flag,
  threads, name length, ticks, resident kB, name (up to 63 bytes); 96
  bytes;
- a snapshot (`SN_*`): the scalars, then 4096 CPU entries of 12 bytes,
  16 disks, 16 interfaces, 16384 processes, then the co-processor
  worker's stats line (`SN_VPU`, 64 bytes: the AVX-512 repository's
  `struct vpu_stats`, whose field offsets are the `VS_*` constants and
  whose place in the card's window is `VPU_OFF_STATS`, 320) and whether
  the sample has it: about 1.6 MB;
- a rate entry (`RATE_*`) and a core entry (`CORE_*`, up to 8 threads);
- the derived record (`DV_*`): interval, mean, four PCIe rates, counts,
  per-CPU loads, cores, device rates, per-process shares, the worker's
  rates (`DV_VPU_*`);
- the history table (`SEEN_*`): 65536 entries of 24 bytes;
- a slot (`SL_*`): index, flags, the two snapshot buffers' roles, the
  error text, name, address, socket path, the derived record, the
  history, two snapshots: about 6.5 MB, sixteen slots in `.bss`, paged
  in as touched.
