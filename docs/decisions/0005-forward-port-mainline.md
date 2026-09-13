# 0005: The card runs a mainline kernel with a KNC platform layer

Status: accepted, 2026-09-13.

## Context

The only kernel known to boot on KNC is Intel's `linux-2.6.38.8+mpss` (and
its 3.5.1 rebuild by the Revival Project). The stated goal is the latest
Linux kernel. The SSDG enumerates exactly what a stock OS must change
(section 4.2), and mainline already contains a precedent for an x86 platform
with no legacy devices (`jailhouse.c`) and the KNC PMU driver.

## Decision

Forward-port: apply a small, reviewable patch series to current mainline
(target: the newest stable series at the time of each phase) that adds a KNC
platform layer and relaxes the SSE/CMOV/PGE assumptions, and build it with
the patched LLVM. Intel's tree is consulted for hardware facts only.

## Consequences

- The series must be re-based per kernel release; kept small on purpose.
- Some Intel-era features (power management, thermal, SCIF) are not
  ported. The card runs at bootstrap-set frequency.
- The first boot target is "kernel prints to the console ring", which does
  not need SMP, networking, or any driver beyond the ring.

## Alternatives rejected

- Boot Intel's 2.6.38 (the XPR-OS route): violates the goal; ancient
  userland ABI constraints (glibc 2.14 era); no modern security or drivers.
- Port Intel's patches file by file: most of them touch code that no longer
  exists in the same shape; the SSDG list is a better specification than the
  diff.
