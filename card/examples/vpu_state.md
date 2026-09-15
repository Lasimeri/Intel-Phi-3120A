# vpu_state.c

Does the kernel preserve the vector unit across context switches? Every
thread loads a pattern unique to it into all 32 zmm registers and the 8
mask registers (`vpu_set` from vpu_probe.S), sleeps a little, reads them
back (`vpu_get`) and compares, in a loop for the given number of seconds.
With two threads per CPU the scheduler switches constantly; with one per
CPU the thread still leaves the CPU for the idle task at every sleep.

```
cc -O2 -o vpu_state vpu_state.c vpu_probe.S -lpthread
./vpu_state THREADS SECONDS        # exit status 1 on any mismatch
```

The report separates mismatches confined to bits 0:127 of the zmm
registers (what FXRSTOR clears on this core, ISA reference 327364-001
B.4) from ones that touch the mask registers.

Results 2026-09-15: kernel without patch 0024, 228 threads for 3 s:
128,214 of 623,255 checks mismatched; 456 threads: 1,231,270 of
1,530,210. With patch 0024 (vector state saved after FXSAVE and restored
after FXRSTOR): see `docs/results/2026-09-15-vpu.md`.
