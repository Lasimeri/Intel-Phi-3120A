# d[i] = a[i]*b[i] + c[i], float32, n a multiple of 16.
# rdi=d  rsi=a  rdx=b  rcx=c  r8=n
    .text
    .globl fma_kernel
fma_kernel:
    test %r8, %r8
    jle  2f
1:
    vmovups (%rsi), %zmm0
    vmovups (%rdx), %zmm1
    vmovups (%rcx), %zmm2
    vfmadd231ps %zmm1, %zmm0, %zmm2
    vmovups %zmm2, (%rdi)
    add $64, %rsi
    add $64, %rdx
    add $64, %rcx
    add $64, %rdi
    sub $16, %r8
    jg 1b
2:
    ret
