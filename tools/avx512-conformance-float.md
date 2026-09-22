# avx512-conformance-float

Three floating-point kernels, run by `scripts/phi512-check.sh`.

| kernel | what it exercises |
| --- | --- |
| `conditional_sum` | a compare, a mask, a multiply and a **divide**, which the card cannot do at all but the host path can |
| `count_and_mask` | an integer mask, a shift, a negate and a select |
| `widen_and_scale` | float32 widened to float64, then a fused multiply-add, then a 64-bit reduction |

`widen_and_scale` is the one that reaches `vpmovsxdq`, `vpaddq` and the
64-bit lane arithmetic; `conditional_sum` is the one that proved the
float path was right while the integer path was still wrong, which
narrowed the search considerably.
