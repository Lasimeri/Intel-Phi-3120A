# phi-vpu: hand AVX-512 work to the card

The host side of the co-processor. Maps the shared window, writes the
data and a request into it, rings the doorbell, waits for the card's
reply, and reads the answer back.

```
phi-vpu status
phi-vpu poly --n 1048576 --threads 57 --repeat 5
```

`poly` evaluates a degree-30 polynomial on the card (the translated
AVX-512 kernel in `card/examples/avx512_poly.S`) and compares **every
returned lane** against what this host's own fused multiply-add hardware
produces through `f32::mul_add`. It reports a speed only for a run whose
lanes all matched, because a fast wrong answer is worth nothing.

## What one request costs, measured

2026-09-22, 57 card threads, the pool warm:

| elements | pull | compute | push | GFLOP/s on the vector units |
| --- | --- | --- | --- | --- |
| 65536 | 1.9 ms | 0.088 ms | 1.6 ms | 45 |
| 1048576 | 8.9 ms | 0.302 ms | 4.2 ms | 208 |
| 4194304 | | 1.035 ms | | 243 |

Compute scales; the transport is the bound. The host-memory block path
serves one 512 KiB record at a time (`docs/results/2026-09-16-dma.md`),
and that is where a request's time goes now. The comparison that matters
for a host with no AVX-512 is against running the same code in software:
`phi512` performs an AVX-512 instruction in about 152 ns once its site is
rewritten, so a million elements of this kernel is roughly 300 ms there
against 14 ms end to end on the card.

## A live worker, not a remembered one

The readiness word stays in the window after a worker dies, and a check
that only reads it is satisfied by a corpse: the request then waits its
full minute for an answer that never comes. `wait_ready` clears the word
first and waits for the card to re-assert it, which a live worker does
on every poll, within a millisecond even while sleeping between polls.
With no worker at all this fails in five seconds and says so.

## The doorbell protocol

The request's fields are written first, then a fence, then the sequence
number, which is the only word the card polls. The card writes its
reply's fields and echoes the sequence number last, so seeing the number
means the rest is there. Sequence numbers start from what the window
holds plus one; the worker zeroes both counters when it starts, so a
request left by an earlier run cannot be mistaken for a new one.

## Layout of a request's data

Every region starts on a 4096-byte block and is followed by its own
slack, because the card moves whole blocks with `O_DIRECT`. Input, then
the coefficients (one copy per lane, as the kernel loads them), then the
output. `proto.rs` has the rules and the reasons.
