# 2026-09-22: the seamless path, tested from the user's side

Question asked: does an ordinary AVX-512 program, run on this host with
no AVX-512, get its instructions intercepted and executed by the Phi?

Answer: **intercepted, yes; executed correctly, yes; by the Phi, no.**
Today the transparent path ends in the host-side software emulator.
The card runs AVX-512 code only through the explicit driver
(`phi-vpu poly`, one compiled-in kernel). Nothing in `libphi512` knows
the card exists (`grep -i card host/crates/phi512/src` finds nothing),
which is the state the handoff note records as items 2 to 4 of its
"next" list; the test confirms it from the outside.

## The test

`tools/avx512-seamless-test.c`: intrinsics, `-mavx512f`, three kernels
(a degree-30 Horner polynomial, a dot product with a reduce, an integer
update through a mask register), each checked bit for bit against a
scalar reference compiled with `target("no-avx512f")` so the reference
cannot be intercepted. 18 AVX-512 instruction sites in the binary.

```
gcc -O2 -mavx512f -mno-avx512vl -mno-avx512bw -mno-avx512dq -mno-avx512cd \
    -o seamless tools/avx512-seamless-test.c -lm
./seamless 65536                      # exit 132: SIGILL, as the 5800X should
scripts/phi512.sh --verbose ./seamless 65536
```

```
phi512: AVX-512 will be performed in software on this host
phi512: performed 18 AVX-512 instructions, rewrote 18 sites (0 too short to rewrite), 0 breakpoints (0 unrecognised)
host: /proc/cpuinfo does NOT list avx512f
polynomial (31 fmadd per vector):    22.518 ms  every lane bit-identical to the scalar reference
dot product (fmadd + reduce):         1.091 ms  bit-identical (-28.6031036)
integers (compare, mask, mullo):      3.406 ms  every lane identical
PASS: the AVX-512 code ran and its answers are right
```

At 1048576 elements: polynomial 376 ms, dot 18.4 ms, integers 56.6 ms,
all bit-identical, PASS.

## Where it ran

Read from the other side before and after both runs:

| | card 0 | card 1 |
| --- | --- | --- |
| VPU worker requests logged | 18 before, 18 after | 18 before, 18 after |
| DMA copies to the card | 3149 before, 3149 after | 2990 before, 2990 after |

The cards did nothing. The "18 instructions" the wrapper reports are 18
static sites, each rewritten once into a call to the emulator and then
executed thousands of times on the host CPU.

## What the same work costs on the card today, explicitly

The polynomial kernel is the one the card has compiled in
(`card/examples/avx512_poly.S`, degree 30, 16 lanes), so the comparison
is direct (`2026-09-22-block-pipeline.md`):

| degree-30 polynomial | host emulation (seamless) | card, explicit, transport included |
| --- | --- | --- |
| 65536 elements | 22.5 ms | 0.50 ms |
| 1048576 elements | 376 ms | 3.0 to 3.5 ms |

Between 45x and 110x. That gap is what wiring the two together is worth.

## What "the Phi intercepts and executes" would take

The pieces exist separately; what is missing is the join, in this order:

1. **Region detection** in `libphi512`: from the first `SIGILL` in a
   loop, find the loop (walk forward to the backward branch) and its
   memory operands (base registers, strides, trip count), instead of
   rewriting one instruction at a time.
2. **Runtime kernels on the card**: the worker takes one kernel id
   (`VPU_K_POLY30`); it needs to accept translated code bytes
   (`avx512-xlate` already produces MVEX for the 1:1 arithmetic) and
   run them over a window region.
3. **Dispatch**: copy the loop's input ranges into the window, submit,
   wait, copy the outputs back into the program's memory, set the
   registers the loop would have left, resume after it. Everything else
   (an instruction outside a loop, an instruction the translator does
   not cover) stays with the emulator.
4. **Profitability**: below about 1000 elements the emulator wins
   (0.5 ms transport floor against 26.5 ms per 65536 elements emulated).

None of this is in the repository yet. Until it is, `scripts/phi512.sh`
is a correct AVX-512 emulator and the card is an explicit accelerator.
