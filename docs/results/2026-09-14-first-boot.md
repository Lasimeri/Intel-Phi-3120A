# 2026-09-14: mainline Linux boots on the card (phase P3 done)

`phictl boot --kernel card/kernel/build/out/arch/x86/boot/bzImage
--cmdline "earlyprintk=phiring,keep loglevel=8 nosmp" --watch 0x2220500`,
host kernel 7.2.3-1-cachyos, card kernel 7.2.3 plus the fifteen-patch
series at commit 3b8e28f (host repo). Full log in `~/phi-t3d.log`.

## What the card printed

- POST trail: bootstrap `50`, `FF`; kernel `K0`, `K8`, `KF`, `KG`,
  marks `KJ`..`KR`, `K9`, `KA`..`KD`, `K2`, `K3`, `K5`, `K4`, `K7`.
- `Hypervisor detected: Knights Corner card (native)`, family 0xb model
  0x1 stepping 0x2; SFI SYST at `0xef180`, 228 CPUs, one I/O APIC at
  `0x8007da800`, 8 memory ranges; bootstrap e820: RAM `0x100000` to
  `0xfedfffff` and `0x100000000` to `0x173ffefff` (6 GB), the LAPIC
  page and `0xff000000`+ reserved.
- `CLK_RATIO` reads 0 on this card; the platform layer assumed 1100 MHz
  and the TSC calibration accepted it (`tsc: Detected 1100.000 MHz`,
  `Switched to clocksource tsc`, `sched_clock: Marking stable`).
- MTRR: 7 variable entries, PAT configured; NX active; FXSAVE in use.
- Zones, percpu, SLUB, RCU, timers, IRQs, alternatives, kprobes, IOMMU
  core, SCSI/USB/ALSA cores, networking, initcalls, workqueues: all ran.
- `Performance Events: knc PMU driver` bound (2 counters, 40 bits).
- Timestamps advance (0.4 s to 2.4 s), so the LAPIC timer works.
- `[Firmware Bug]: CPU 0: APIC ID mismatch. CPUID: 0x00e0 APIC: 0x0070`:
  the LAPIC ID register holds the ID one bit higher than xAPIC's field
  (SSDG 4.2.4 "expanded ID fields"); harmless under `nosmp`, required
  reading for SMP.

## Card-side bugs found by the boot ladder, all fixed in the series

| Symptom | Cause | Fix |
| --- | --- | --- |
| Silent stop after `KQ` | `x86_64_start_kernel` flushes the trampoline TLB by toggling CR4.PGE (#GP, no IDT: triple fault) | CR3 reload on KNC (0002); PGE cleared in early Intel init (0008) |
| BUG at jump_label.c:25 in `sched_init` | 5-byte NOP was two instructions; the site decoder demands one | `66 66 66 66 90` in nops.h and objtool (0006) |
| #GP in `fpu__init_cpu`, CR4 0x30 to 0x230 | CR4.OSFXSR faults on the card | not set under X86_KNC (0002) |
| No `K9`, no `KI` | my own mark faulted into the empty bringup IDT | removed |

Host-side, the same day: every host reset was caused by writing the
aperture during GDDR retraining after VFIO's open-time reset; fixed by
keying `wait_ready` on POST `12` (`docs/results/2026-09-13-p3-kernel-build.md`).

## Not done

- No initramfs and no input path: the ring console is printk only. P4
  begins with a tty driver on the ring and a busybox initramfs.
- SMP: 227 threads rejected by `nosmp`; the APIC ID field needs the
  expanded decode before INIT/SIPI can target them.
- The LAPIC timer clock and the core clock decode are unverified
  (assumed 1100 MHz; timestamps look plausible but were not measured).
