# tcc on the card

tcc's `x86_64-gen.c` emits SSE for every floating-point operation
(`movsd`, `addsd`, `cvtsi2sd`, `xmm` argument registers per the SysV ABI).
Its `i386-gen.c` uses the x87 stack. The port:

1. In `x86_64-gen.c`, route `gen_opf`, `gen_cvt_itof`, `gen_cvt_ftoi`,
   `gen_cvt_ftof`, and the load/store of `VT_FLOAT`/`VT_DOUBLE` values to
   x87 sequences borrowed from `i386-gen.c` (`fld`, `fstp`, `fild`, `fistp`
   with a control-word round-toward-zero dance for conversions).
2. In `gfunc_call`/`gfunc_prolog`, classify float/double as stack-passed
   and return in `st0`, matching the knc64-x87 ABI.
3. `long double` already uses x87 in tcc; unchanged.
4. tcc emits no CMOV and no fences. Its `libtcc1.a` runtime has SSE in
   `alloca86_64.S`? No: integer only. Audit confirms.

Then `tcc -run hello.c` on the card is the fastest edit-run loop the card
has, which is the point of including it.
