# vendor/

Reference material only. Populated by `scripts/fetch-vendor.sh`, ignored by
git, never linked into any build. Contents after a fetch:

| Directory | Source | Used for |
| --- | --- | --- |
| `mpss-3.8.6/` | archive.org item `intel-mpss-3.8.6` | Host driver source (`mpss-modules`), card boot files, SBOX register headers (`micsboxdefine.h`), `mpssd` command-line generation |
| `solros/phi-kernel/` | github.com/cosmoss-jigu/solros | Intel's `linux-2.6.38.8+mpss3.5.1` tree with K1OM `arch/x86` support. The diff against vanilla 2.6.38.8 is the reference for the mainline forward-port |
| `linux-5.9-mic/` | github.com/torvalds/linux tag v5.9, `drivers/misc/mic` | Last mainline Xeon Phi host driver; register offsets and boot sequence |
| `docs/` | Intel PDFs (ISA reference 327364, SSDG 328207, datasheet, k1om psABI) | Cited by section number throughout `docs/research/` |

Do not copy code out of here into the repository. Read it, cite it, reimplement.
