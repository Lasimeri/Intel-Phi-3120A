# AVX-512 on Knights Corner: what translates bit-exactly and what does not

Question this note answers: can a Xeon Phi 3120A execute AVX-512 instructions
on behalf of a host that has none, and produce the bits AVX-512 hardware would
have produced?

Primary sources: Intel Xeon Phi Coprocessor Instruction Set Architecture
Reference Manual 327364-001 (September 2012), and the System Software
Developer's Guide 328207-002. Text extracted from the PDFs in `vendor/docs`
with `pdftotext -layout`.

The short answer: **yes for the IEEE-defined subset, which is most of
AVX-512F, after two instruction families are synthesised and one is
re-expressed.** The rest of this note is the evidence.

## The two instruction sets are not the same set

KNC's vector ISA is IMCI, encoded with the MVEX prefix. AVX-512 is encoded
with EVEX. Both are 62H-prefixed 4-byte forms, both address 32 512-bit `zmm`
registers and 8 mask registers, and the family resemblance is real: MVEX is
the direct ancestor of EVEX. They are not binary compatible and the
instruction inventories differ.

The KNC vector chapter of 327364-001 documents **113 instruction entries**
(counted from the chapter 6 table of contents). AVX-512F alone has more, and
AVX-512 as a whole has far more.

## The precision audit

This is the part that decides whether the project is possible at all. An
emulator that is fast but wrong is worth nothing, and the competition here is
Intel SDE, which is correct.

### Rounding and denormals: identical controls

| Control | AVX-512 | KNC | Source |
| --- | --- | --- | --- |
| Rounding mode, dynamic | `MXCSR.RC` | `MXCSR.RC`, bits 14-13 | 327364-001 table 2.15 |
| Rounding mode, per instruction | `{er}` in EVEX | Static Rounding Mode, `EH=1` plus the `SSS` swizzle field | 327364-001 section 2.3, table 2.14 |
| All four IEEE modes | yes | yes, `{rn} {rd} {ru} {rz}` | 327364-001 section 2.3 |
| Suppress all exceptions | `{sae}` | SAE attribute on the same encoding | 327364-001 section 4.1.1 |
| Denormal inputs | `MXCSR.DAZ` | `MXCSR.DAZ`, bit 6 | 327364-001 table 2.15 |
| Tiny results | `MXCSR.FZ` | `MXCSR.FZ`, bit 15 | 327364-001 table 2.15 |

AVX-512's embedded rounding is KNC's static rounding mode with a different
encoding. This maps one to one.

### Core arithmetic: bit-exact

327364-001 appendix C.3 lists, per instruction, how denormals are treated.
For every basic arithmetic instruction the answer is `MXCSR.DAZ` for inputs
and `MXCSR.FZ` for tiny results, which is the AVX-512 rule:

`vaddpd vaddps vsubpd vsubps vmulpd vmulps`, and all twelve FMA forms
`vfmadd132/213/231` and `vfmsub`, `vfnmadd`, `vfnmsub` in `pd` and `ps`.

With `DAZ=0` and `FZ=0` these are IEEE-754 operations with the selected
rounding mode, on both machines, on the same 512-bit lane layout. They are
bit-exact by construction. The FMA is fused on both, single-rounded.

**This is the bulk of AVX-512F floating point and it needs no emulation at
all, only re-encoding.**

### Divide and square root: absent from the card

`VDIVPS`, `VDIVPD`, `VSQRTPS`, `VSQRTPD` do not exist on Knights Corner. This
is not a sparse grep, it is an empty one: the strings `vdiv` and `vsqrt` do
not occur anywhere in 327364-001.

What the card offers instead:

| Instruction | Accuracy | Source |
| --- | --- | --- |
| `VRCP23PS` | 0.912 ULP relative error | 327364-001 p577 |
| `VRSQRT23PS` | 0.775 ULP relative error | 327364-001 p588 |

Both are float32 only. There is no float64 reciprocal or reciprocal square
root on the card at all (`VRCP23PD` and `VRSQRT23PD` do not exist).

So a bit-exact `vdivps` must be synthesised: reciprocal seed, Newton-Raphson
refinement, then a residual correction with FMA to force the correctly
rounded result. Correctly rounded division from an approximate reciprocal is
a solved problem and the card has the FMA needed for the residual step.

Float64 divide and square root are the expensive case, because the seed has
to come from the float32 unit: convert down with `vcvtpd2ps`, take
`vrcp23ps`, convert back with `vcvtps2pd`, then refine in float64. Each
Newton-Raphson step doubles the correct bits, so a 23-bit seed reaches 53
bits in two steps, plus the residual fixup.

Estimated expansion: roughly 6 to 10 instructions for float32, 12 to 16 for
float64. These are the most expensive instructions in the translation and
they are the reason a divide-heavy kernel will not be representative of
overall cost.

### The approximation instructions: no bit-exactness exists to preserve

AVX-512's `VRCP14PS` and `VRSQRT14PS` are specified by an error bound, less
than 2 to the minus 14, not by an exact result. Intel does not architecturally
define their bits, so two AVX-512 implementations are free to disagree and
there is no reference answer for a translator to reproduce.

KNC's equivalents are quoted by 327364-001 in its own units, "0.912ULP
(relative error)" and "a precision of 0.775ULP (relative error)". That
phrasing mixes two scales and is not directly comparable to AVX-512's
2 to the minus 14 bound, so no claim is made here about which is tighter.
It does not matter for scope: since AVX-512 does not define the bits, there
is no reference answer to reproduce, and these instructions are therefore
documented as approximate on both sides rather than bit-exact.

### Minimum and maximum: different NaN rule

| | Rule on NaN |
| --- | --- |
| AVX-512 `VMAXPS` | returns the second source |
| KNC `VGMAXPS` | returns the operand that is not NaN (IEEE-754-2008 `maxNum`) |

327364-001 p-VGMAXPS states it explicitly: "If one source operand is NaN, then
the other source operand is returned", and notes it deliberately differs from
the SSE rule.

These are not interchangeable, but the AVX-512 rule is two KNC instructions:
an ordered greater-than compare, which is false when either operand is NaN,
then a blend. `VCMPPS` and `VPBLENDMD` both exist. This reproduces the
AVX-512 result including the NaN and the signed-zero cases.

### Compare predicates: 8 encodable against 32

KNC `VCMPPS` encodes 3 predicate bits, 8 values, reaching 12 logical
predicates by swapping operands (327364-001 table 6.4). AVX-512 `VCMPPS`
takes a 5-bit immediate with 32 predicates, being the 8 classic ones crossed
with ordered/unordered and quiet/signalling variants.

The result bits of all 32 are reachable on the card. What is not always
reachable is the exact set of MXCSR exception flags each variant raises,
since the quiet and signalling distinction is precisely about which
predicates signal on a QNaN. A translator that reproduces result bits but not
the invalid flag is correct for every program that does not read MXCSR after a
compare, which is nearly all of them, and this limitation is recorded rather
than hidden.

Note also that KNC's `VCMPPS` writes zeros into masked-off destination bits
instead of leaving them, which the translator must account for when a compare
is itself write-masked.

### Mask registers: 16 bits, which is exactly what AVX-512F needs

327364-001 section 2.1.2: "a vector register holds either 8 or 16 elements;
accordingly, the length of a vector mask register is 16 bits. For 64 bit
datatype instructions, only the 8 least significant bits of the vector mask
register are used."

AVX-512F operates on dword and qword elements only, so it uses 16 and 8 mask
bits respectively. The widths agree. AVX-512's `k` registers are
architecturally 64 bits wide, but only AVX-512BW and DQ define operations
that reach above bit 15, and those extensions are already out of scope. A
translator confined to AVX-512F never needs a bit the card does not have.

### Gather and scatter are not single-instruction-complete

This one changes the shape of the translator's output.

AVX-512's `VGATHERDPS` is architecturally one instruction that completes:
it services every enabled element and clears the mask. KNC's is not.
327364-001 p300 states it directly:

> Note the special mask behavior as only a subset of the active elements of
> write mask k1 are actually operated on. There are only two guarantees about
> the function: (a) the destination mask is a subset of the source mask, and
> (b) on a given invocation of the instruction, at least one element will be
> selected from the source mask.
>
> Programmers should always enforce the execution of a gather/scatter
> instruction to be re-executed (via a loop) until the full completion of the
> sequence (i.e. all elements of the gather/scatter sequence have been
> loaded/stored and hence, the write-mask bits all are zero).

So one AVX-512 gather becomes a KNC **loop**: copy the mask, gather, test the
mask, branch back while it is non-zero. Forward progress is guaranteed by
point (b), at least one element per iteration, so the loop terminates in at
most 16 iterations.

The consequence for the design: **a translated region cannot be modelled as
straight-line code.** The translator emits basic blocks with internal
backedges, and any region containing a gather or a scatter has control flow
that the original AVX-512 region did not. This has to be in the region model
from the start rather than retrofitted.

Note also `VGATHERDPS` faults if the destination register is the same as the
index register, which a register allocator has to respect.

## What cannot be translated at all

| AVX-512 feature | Why | Workaround |
| --- | --- | --- |
| AVX-512BW, all byte and word integer vector ops | KNC has no byte or word vector integer instructions of any kind. The integer set is dword and qword only. | widen to dword, operate, narrow; 4x the lanes, so 4x the instructions, and only for kernels where that is semantically sound |
| AVX-512VL, the 128-bit and 256-bit forms | KNC vector instructions are 512-bit only. There is no `xmm` or `ymm` register file; 327364-001 appendix B.2 lists every MMX, XMM and YMM instruction as unsupported. | operate at 512 bits with a write-mask limiting the active lanes |
| `VPTERNLOGD` and `VPTERNLOGQ` | not in the KNC inventory | expand the 8-bit truth table into the KNC logic ops; worst case a few instructions, and it is a compile-time constant so the expansion is chosen once |
| AVX-512DQ, IFMA, VBMI, VNNI, BF16, FP16 | postdate the card by years | out of scope |

## The instruction inventory, as extracted

The 113 documented KNC vector and scalar-extension entries:

```
CLEVICT0 CLEVICT1 DELAY LZCNT POPCNT SPFLT TZCNT TZCNTI VGMAXABSPS VGMAXPD
VGMAXPS VGMINPD VGMINPS VLOADUNPACKHD VLOADUNPACKHPD VLOADUNPACKHPS
VLOADUNPACKHQ VLOADUNPACKLD VLOADUNPACKLPD VLOADUNPACKLPS VLOADUNPACKLQ
VLOG2PS VMOVAPD VMOVAPS VMOVDQA32 VMOVDQA64 VMOVNRAPD VMOVNRAPS VMOVNRNGOAPD
VMOVNRNGOAPS VMULPD VMULPS VPACKSTOREHD VPACKSTOREHPD VPACKSTOREHPS
VPACKSTOREHQ VPACKSTORELD VPACKSTORELPD VPACKSTORELPS VPACKSTORELQ VPADCD
VPADDD VPADDSETCD VPADDSETSD VPANDD VPANDND VPANDNQ VPANDQ VPBLENDMD
VPBLENDMQ VPBROADCASTD VPBROADCASTQ VPCMPD VPCMPEQD VPCMPGTD VPCMPLTD VPCMPUD
VPERMD VPERMF32X4 VPGATHERDD VPGATHERDQ VPMADD231D VPMADD233D VPMAXSD VPMAXUD
VPMINSD VPMINUD VPMULHD VPMULHUD VPMULLD VPORD VPORQ VPREFETCH0 VPREFETCH1
VPREFETCH2 VPREFETCHE0 VPREFETCHE1 VPREFETCHE2 VPREFETCHENTA VPREFETCHNTA
VPSBBD VPSBBRD VPSCATTERDD VPSCATTERDQ VPSHUFD VPSLLD VPSLLVD VPSRAD VPSRAVD
VPSRLD VPSRLVD VPSUBD VPSUBRD VPSUBRSETBD VPSUBSETBD VPTESTMD VPXORD VPXORQ
VRCP23PS VRNDFXPNTPD VRNDFXPNTPS VRSQRT23PS VSCALEPS VSCATTERDPD VSCATTERDPS
VSCATTERPF0DPS VSCATTERPF0HINTDPD VSCATTERPF0HINTDPS VSCATTERPF1DPS VSUBPD
VSUBPS VSUBRPD VSUBRPS
```

`VPERMD` is present, which an earlier guess in this project had wrong. The
`VSUBR` and `VPSUBR` reversed-operand forms, `VPADC`/`VPSBB` carry-propagating
adds, and `VSCALEPS` are KNC extras with no AVX-512 counterpart; they are
useful as translation targets even though nothing maps to them.

Unaligned access deserves its own line, because AVX-512 code uses it
constantly and the card has no `vmovups`. KNC splits an unaligned 512-bit
access into a pair: `VLOADUNPACKLD` plus `VLOADUNPACKHD` to load, and
`VPACKSTORELD` plus `VPACKSTOREHD` to store. Two instructions per unaligned
access, always, and the two halves address consecutive cache lines.

## Width, and what "each VPU takes part of an instruction" would mean

A KNC VPU is **512 bits wide**, the same width as an AVX-512 instruction: 16
float32 lanes or 8 float64 lanes, in both machines. One VPU therefore covers
one entire AVX-512 instruction's worth of lanes, exactly, with nothing left
over.

Splitting a single AVX-512 instruction across several VPUs would hand each
one a fraction of a lane and require cross-core coordination over the ring,
which costs hundreds to thousands of cycles, in order to save at most a
single cycle of VPU time. It is a loss of about three orders of magnitude,
and it is not what makes the card act as one unit.

What does make the card act as one unit is splitting the **data** the
instruction stream runs over. The card has 57 cores, one VPU each, so:

| | float32 lanes | float64 lanes |
| --- | --- | --- |
| one KNC VPU | 16 | 8 |
| the whole card, 57 VPUs | **912** | **456** |
| host 5800X, widest available unit, AVX2 | 64 | 32 |
| host 5800X, AVX-512 | 0 | 0 |

Peak vector rate at 1.1 GHz with FMA counted as two operations: 57 x 1.1e9 x
16 x 2 = **2.006 TFLOP/s float32**, half that for float64.

One constraint governs how those VPUs must be driven. From SSDG 328207-002
section 2.1.2: the instruction decoder is a two-cycle unit, so "the core
cannot issue instructions from the same hardware context in back-to-back
cycles", and therefore "for maximum chip utilization, at least two hardware
contexts or threads must be run on each core. Running one thread on a core
will result in, at best, 50% utilization of the core potential."

**So the unit of dispatch is 114 threads minimum, 228 to also hide memory
latency, driving 57 VPUs.** A design that puts one thread on each core
throws away half the machine before it starts.

### The floor case, stated before anyone benchmarks it

The 912-lane figure applies to a region that is loop-shaped with independent
iterations, because that is what lets 57 VPUs work on disjoint slices. Two
cases have to be kept apart:

| Region shape | VPUs usable | Expectation |
| --- | --- | --- |
| one intercepted AVX-512 instruction, or straight-line code over a single 512-bit operand set | **1** | slower than the host would have been, by roughly the clock ratio, and irreducibly so. There are no slices to spread. |
| an AVX-512 loop over N elements with independent iterations | **57**, driven by 114 to 228 threads | the unified co-processor case, 912 float32 lanes |

The first row is not a defect to be optimised away, it is arithmetic: a
512-bit operation has 512 bits of work in it, one VPU is 512 bits wide, and
1.1 GHz in-order is slower than 4.5 GHz out-of-order. Any measurement that
intercepts straight-line AVX-512 will show a regression, and that is the
expected result rather than a surprise.

## Verdict

Bit-exact AVX-512F execution on this card is possible. The work is:

1. re-encode EVEX to MVEX for the arithmetic that already matches, which is
   most of it,
2. synthesise divide and square root, the only two IEEE operations the card
   lacks,
3. re-express min, max and the wider compare predicate set,
4. expand unaligned access into load-unpack and pack-store pairs,
5. declare AVX-512BW, VL and everything after AVX-512F out of scope, with
   AVX-512BW reachable by widening where a kernel allows it.

The approximation instructions are inside the AVX-512 error bound but not bit
identical, because AVX-512 does not define their bits.

Related: `isa-deletions.md`, `abi-and-toolchain.md`.
