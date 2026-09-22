# avx512-xlate manifest

Two dependencies carry the crate. `iced-x86` decodes EVEX, which is a real
decoder for the whole of x86-64 rather than something written here: AVX-512
encoding has enough corners (compressed disp8, the `b` bit meaning broadcast
on memory operands and rounding control on register ones, the `L'L` length
field) that a hand-rolled decoder would be a source of silent wrong answers.
It is already a workspace dependency, used by `phi-isa-audit`.

`knc-mvex` does the encoding, and its output is verified on the card rather
than against an assembler, because no assembler for this vector ISA exists.
