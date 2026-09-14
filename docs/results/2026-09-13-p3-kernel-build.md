# 2026-09-13: card kernel builds and audits clean (phase P3, build half)

Host: Arch/CachyOS 7.2.3-1-cachyos, 16 threads. Kernel: stable v7.2.3
plus the fifteen-patch series in `card/kernel/patches/` (`SERIES`),
configured from `x86_64_defconfig` + `card/kernel/config/knc.config`,
compiled by `card/kernel/build.sh` with `CC=knc-cc` (patched LLVM 22.1.8,
patches 0001 to 0009; 0009 is new: clang `-mno-cmov`/`-mno-nopl` driver
flags so the deletion list reaches the integrated assembler for `.S`
files).

```
card/kernel/build.sh patch      15 patches applied with git am onto v7.2.3
card/kernel/build.sh configure  configuration matches the fragment
card/kernel/build.sh build      bzImage 8.7 MB (about 6 minutes clean)
card/kernel/build.sh audit
vmlinux: 4 executable section(s) [.text, .init.text, .altinstr_aux, .exit.text], 5281163 instructions
result: 0 illegal, 0 suspect, 69 ignored
```

## What the audit found on the way (each now a patch, a config line, or a documented exception)

| Count | Instruction | Where | Resolution |
| --- | --- | --- | --- |
| 6313 before, 0 after | | | |
| ~4000 | SSE/AVX/AVX-512/SHA-NI/PCLMUL/BMI | `lib/crypto` x86 assembly, `lib/crc` PCLMUL, DRM `movntdqa` | patch 0014 (`lib/crypto` defaults off under `X86_KNC`), `CRC_OPTIMIZATIONS=n`, `DRM=n` |
| 92 per object | `0F 1F` multi-byte NOPs in C objects | objtool's jump-label rewriting uses its own NOP table | patch 0006 (objtool table) |
| 21 | multi-byte NOPs in `.S` objects | assembler alignment; `-Xclang -target-feature` never reached cc1as | LLVM patch 0009 + `knc-cc -mno-cmov -mno-nopl` |
| every user access | `cmova` | inline asm in `mask_user_address()` and `getuser.S`; `cmovzl` via `CONFIG_X86_CMOV` | patch 0013 |
| 27 | `rdpkru`/`wrpkru` | protection keys | `X86_INTEL_MEMORY_PROTECTION_KEYS=n` |
| 27 | `lzcnt` | zstd BMI2 decoder variants | `RD_ZSTD=n` |
| 14 | `movnti`, `sfence` | `__copy_user_nocache` | patch 0015 |
| 2 | `prefetcht0` | `copy_page_regs`, `csum_partial_copy_generic` (both executed on the card) | patch 0004 |
| 3 | `mfence`/`lfence` | `weak_wrmsr_fence()`, `clear_bhb_loop`, TSC-deadline setup | patch 0003 |
| 6 | `movnti` in `__memcpy_flushcache` | `ARCH_HAS_UACCESS_FLUSHCACHE` | patch 0007 |
| 69 | FSGSBASE, CMPXCHG16B, MONITOR/MWAIT, XSAVE/XSAVES, AMX, INVPCID, SERIALIZE, WAITPKG, RDRAND/RDSEED, one `emms`, VMware port I/O | each behind a CPUID or erratum check the static audit cannot follow | `--ignore` list in `build.sh`, one comment per reason |
| n/a | `clac`/`stac`/`lfence` | only inside `.altinstr_replacement` | `phi-isa-audit` now skips that section by name |

Build failures fixed on the way: an `asm` operand must be a string
literal (0006), `BYTES_NOP9..11` are required by `alternative.c` (0006),
`e820__print_table` is static (0012), `X86_KNC` selecting
`HYPERVISOR_GUEST` created a Kconfig recursion through `X86_X2APIC`
(0001: `depends on`), `MICROCODE` is `def_bool y` (0008 disables the
loader at run time), `clear_bhb_loop` is referenced by alternatives even
without the BHI mitigation (0003 substitutes its fence instead).

## Readings

- The kernel text for the card contains no instruction from the deletion
  list outside the 69 documented, CPUID-guarded sites. Alternatives and
  static keys explain every one of those; none is reachable on a CPU
  whose CPUID lacks the feature.
- Two residues would have executed on every boot and were only visible
  because the audit runs on the linked image: `prefetcht0` in
  `copy_page_regs` (the path taken once REP_GOOD is cleared) and in the
  checksum copy used by networking.
- Inline assembly, objtool and the assembler each bypass the compiler's
  feature flags in their own way; the audit caught all three.

## Not done

- Nothing has booted. Next: `sudo host/target/debug/phictl boot --kernel
  card/kernel/build/out/arch/x86/boot/bzImage` (default command line
  `earlyprintk=phiring,keep loglevel=8`; the loader appends `memmap=` and
  `phi.ring=`), watching POST codes `K0`..`K7` and the ring console. The
  card is shared with the other session (VFIO single-open).
- SMP bring-up is untested (Intel's card kernel used the standard
  INIT/SIPI sequence).
