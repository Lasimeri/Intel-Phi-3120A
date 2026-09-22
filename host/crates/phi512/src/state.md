# The imaginary register file

`VState` is the AVX-512 architectural state this host does not have: 32
vector registers of 512 bits, and 8 mask registers.

It is held as bytes rather than as typed lanes because the same 64 bytes
are read as float32, float64, int32 or int64 depending on which instruction
touches them. Reinterpreting bytes is exactly what the hardware does, so
storing bytes and casting on access is the honest model; storing `f32`
would make `vpaddd` on the same register awkward and would invite a bug
where a signalling NaN is quietened by passing through a float variable.

Zero-initialised, which is what the kernel gives a thread when it first
establishes vector state.

## `lane_enabled`

`k0` means "no mask", and enables every lane. That is the architectural
meaning of encoding zero in an instruction's mask field, not a convenience
invented here: `k0` cannot be used as a write-mask, so the encoding is free
to mean "unmasked". Getting this backwards would disable every lane of
every unmasked instruction, which is the sort of thing that fails loudly
and immediately, but it is worth saying why the check reads the way it does.
