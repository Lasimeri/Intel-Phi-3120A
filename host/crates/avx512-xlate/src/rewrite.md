# rewrite.rs

Byte-level EVEX to MVEX rewriting for the seamless path, where the
instruction must keep its length and its memory operand exactly (the
compiler's indexed, scaled and RIP-relative addressing, which the
builder in `lib.rs` cannot express).

The card's MVEX prefix and AVX-512's EVEX prefix share the four-byte
shape, the opcode maps, and the ModRM, SIB and displacement bytes,
including the disp8*N scaling. For the instructions the card has one to
one the rewrite is the prefix payload: P1 bit 2 clears, P2's `z L'L b
V' aaa` becomes `E SSS V' aaa`, SSS 001 for a memory broadcast. The
whitelist `one_to_one` names each card instruction by its opcode line in
the ISA reference 327364-001: moves, the float arithmetic and fused
multiply-adds, the integer arithmetic and logic, shifts by immediate,
broadcasts, the compares into masks (the eight predicates are numbered
the same), permutes, min and max.

A few instructions become out-of-line sequences (`Rewrite::Thunk`): the
site becomes `jmp rel32` into a thunk area, the sequence runs, and jumps
back (`thunk_bytes` lays it out). `vextractf64x4` and `vextractf32x4`
are a 128-bit block permute and a masked zero of the upper lanes, with
k7 and rax saved on the program's stack around it; `vpternlogd` with the
all-ones idiom is a broadcast of a constant placed, 64-byte aligned (the
card faults on a misaligned vector operand), after the jump back;
unaligned moves are the card's unpack and pack pairs with the original
memory operand and the same operand at +64.

Everything else is refused with a reason, and the region builder treats
a refusal as a region boundary, except at the faulting instruction
itself, where it is an error the program cannot get past. Zeroing
masking, embedded rounding, the 128-bit and 256-bit forms, gathers,
divides and square roots are among them.

Checked on the card by running `tools/avx512-seamless-test.c` through
`scripts/phi512.sh` (2026-09-22): every lane bit-identical.
