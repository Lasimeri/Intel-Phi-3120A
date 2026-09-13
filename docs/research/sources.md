# Sources

## Intel documents

| Document | Number | Where obtained |
| --- | --- | --- |
| Xeon Phi Coprocessor Instruction Set Architecture Reference Manual | 327364-001 | intel.com `/content/dam/develop/external/us/en/documents/327364001en.pdf` |
| Xeon Phi Coprocessor System Software Developers Guide | 328207-002 | hep.ph.liv.ac.uk mirror and kib.kiev.ua mirror |
| Xeon Phi Coprocessor Datasheet | 328209-002 | intel.com and mines.edu mirror |
| Xeon Phi Coprocessor x100 Specification Update | | intel.com |
| System V ABI, K1OM Architecture Processor Supplement 1.0 | | community.intel.com attachment `k1om-psabi-1.0.pdf` |
| Intel ARK: Xeon Phi 3120A (SKU 75797), 3120P (SKU 75798) | | intel.com |
| MPSS 2.1 readme (POST codes) | intc_dd_mic_2.1.6720-16 | delivery04.dhe.ibm.com mirror |

## Source trees

| Tree | Location |
| --- | --- |
| Linux mainline (master, and tag v5.9 for `drivers/misc/mic`) | github.com/torvalds/linux |
| Intel k1om kernel `linux-2.6.38.8+mpss3.5.1` | github.com/cosmoss-jigu/solros, directory `phi-kernel` |
| MPSS 3.8.6 archives | archive.org/details/intel-mpss-3.8.6 |
| `mpss-modules` forks | github.com/subgeniuskitty/xeon-phi-kernel-module, github.com/charlieporth1/mpss-modules, github.com/jjkeijser/mpss, github.com/CIRCL/mpss-modules, github.com/huanzhang12/mpss-modules |
| GCC 5.1.1 k1om cross recipe | github.com/apc-llc/gcc-5.1.1-knc |
| Xeon Phi Revival Project | github.com/Xeon-Phi-Revival-Project/xeon-phi-revival |
| LLVM `X86CallingConv.td` | github.com/llvm/llvm-project |
| Rust target specs | github.com/rust-lang/rust, `compiler/rustc_target/src/spec/targets/` |

## Databases and threads

- linux-hardware.org device page `pci:8086-225d-8086-3c98` (subsystem decode).
- cateee.net LKDDb entry for `CONFIG_INTEL_MIC_HOST`.
- LLVM-dev, December 2013, "JIT on Intel KNC"; July 2013, "LLVM x86 backend for Intel MIC".
- community.intel.com: "Knights Corner" ISA thread (2012); "Are there any instructions in k1om can replace lfence"; "Got to work Python as a native application" (2013); "GCC OpenMP 4.0 Offloading to a Real Knights Corner Xeon Phi Card".
- python-dev, January 2016, "python3 k1om dissociation permanence"; bugs.python.org issue 26193.
- Bun documentation `docs/installation.mdx` (CPU requirements); oven-sh/bun issues 14745 and 14905.
- web.eece.maine.edu/~vweaver/projects/mic (PAPI/perf on KNC).
