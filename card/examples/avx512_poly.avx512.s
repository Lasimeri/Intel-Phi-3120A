# poly_kernel_x8: eight independent Horner chains, enough ready
# instructions to cover the vector unit's multi-cycle result latency.
    .text
    .globl poly_kernel_x8
poly_kernel_x8:
    test %rcx, %rcx
    jle 2f
1:
    vmovaps 0(%rsi), %zmm8
    vmovaps 64(%rsi), %zmm9
    vmovaps 128(%rsi), %zmm10
    vmovaps 192(%rsi), %zmm11
    vmovaps 256(%rsi), %zmm12
    vmovaps 320(%rsi), %zmm13
    vmovaps 384(%rsi), %zmm14
    vmovaps 448(%rsi), %zmm15
    vmovaps (%rdx), %zmm0
    vmovaps (%rdx), %zmm1
    vmovaps (%rdx), %zmm2
    vmovaps (%rdx), %zmm3
    vmovaps (%rdx), %zmm4
    vmovaps (%rdx), %zmm5
    vmovaps (%rdx), %zmm6
    vmovaps (%rdx), %zmm7
    vfmadd213ps 64(%rdx), %zmm8, %zmm0
    vfmadd213ps 64(%rdx), %zmm9, %zmm1
    vfmadd213ps 64(%rdx), %zmm10, %zmm2
    vfmadd213ps 64(%rdx), %zmm11, %zmm3
    vfmadd213ps 64(%rdx), %zmm12, %zmm4
    vfmadd213ps 64(%rdx), %zmm13, %zmm5
    vfmadd213ps 64(%rdx), %zmm14, %zmm6
    vfmadd213ps 64(%rdx), %zmm15, %zmm7
    vfmadd213ps 128(%rdx), %zmm8, %zmm0
    vfmadd213ps 128(%rdx), %zmm9, %zmm1
    vfmadd213ps 128(%rdx), %zmm10, %zmm2
    vfmadd213ps 128(%rdx), %zmm11, %zmm3
    vfmadd213ps 128(%rdx), %zmm12, %zmm4
    vfmadd213ps 128(%rdx), %zmm13, %zmm5
    vfmadd213ps 128(%rdx), %zmm14, %zmm6
    vfmadd213ps 128(%rdx), %zmm15, %zmm7
    vfmadd213ps 192(%rdx), %zmm8, %zmm0
    vfmadd213ps 192(%rdx), %zmm9, %zmm1
    vfmadd213ps 192(%rdx), %zmm10, %zmm2
    vfmadd213ps 192(%rdx), %zmm11, %zmm3
    vfmadd213ps 192(%rdx), %zmm12, %zmm4
    vfmadd213ps 192(%rdx), %zmm13, %zmm5
    vfmadd213ps 192(%rdx), %zmm14, %zmm6
    vfmadd213ps 192(%rdx), %zmm15, %zmm7
    vfmadd213ps 256(%rdx), %zmm8, %zmm0
    vfmadd213ps 256(%rdx), %zmm9, %zmm1
    vfmadd213ps 256(%rdx), %zmm10, %zmm2
    vfmadd213ps 256(%rdx), %zmm11, %zmm3
    vfmadd213ps 256(%rdx), %zmm12, %zmm4
    vfmadd213ps 256(%rdx), %zmm13, %zmm5
    vfmadd213ps 256(%rdx), %zmm14, %zmm6
    vfmadd213ps 256(%rdx), %zmm15, %zmm7
    vfmadd213ps 320(%rdx), %zmm8, %zmm0
    vfmadd213ps 320(%rdx), %zmm9, %zmm1
    vfmadd213ps 320(%rdx), %zmm10, %zmm2
    vfmadd213ps 320(%rdx), %zmm11, %zmm3
    vfmadd213ps 320(%rdx), %zmm12, %zmm4
    vfmadd213ps 320(%rdx), %zmm13, %zmm5
    vfmadd213ps 320(%rdx), %zmm14, %zmm6
    vfmadd213ps 320(%rdx), %zmm15, %zmm7
    vfmadd213ps 384(%rdx), %zmm8, %zmm0
    vfmadd213ps 384(%rdx), %zmm9, %zmm1
    vfmadd213ps 384(%rdx), %zmm10, %zmm2
    vfmadd213ps 384(%rdx), %zmm11, %zmm3
    vfmadd213ps 384(%rdx), %zmm12, %zmm4
    vfmadd213ps 384(%rdx), %zmm13, %zmm5
    vfmadd213ps 384(%rdx), %zmm14, %zmm6
    vfmadd213ps 384(%rdx), %zmm15, %zmm7
    vfmadd213ps 448(%rdx), %zmm8, %zmm0
    vfmadd213ps 448(%rdx), %zmm9, %zmm1
    vfmadd213ps 448(%rdx), %zmm10, %zmm2
    vfmadd213ps 448(%rdx), %zmm11, %zmm3
    vfmadd213ps 448(%rdx), %zmm12, %zmm4
    vfmadd213ps 448(%rdx), %zmm13, %zmm5
    vfmadd213ps 448(%rdx), %zmm14, %zmm6
    vfmadd213ps 448(%rdx), %zmm15, %zmm7
    vfmadd213ps 512(%rdx), %zmm8, %zmm0
    vfmadd213ps 512(%rdx), %zmm9, %zmm1
    vfmadd213ps 512(%rdx), %zmm10, %zmm2
    vfmadd213ps 512(%rdx), %zmm11, %zmm3
    vfmadd213ps 512(%rdx), %zmm12, %zmm4
    vfmadd213ps 512(%rdx), %zmm13, %zmm5
    vfmadd213ps 512(%rdx), %zmm14, %zmm6
    vfmadd213ps 512(%rdx), %zmm15, %zmm7
    vfmadd213ps 576(%rdx), %zmm8, %zmm0
    vfmadd213ps 576(%rdx), %zmm9, %zmm1
    vfmadd213ps 576(%rdx), %zmm10, %zmm2
    vfmadd213ps 576(%rdx), %zmm11, %zmm3
    vfmadd213ps 576(%rdx), %zmm12, %zmm4
    vfmadd213ps 576(%rdx), %zmm13, %zmm5
    vfmadd213ps 576(%rdx), %zmm14, %zmm6
    vfmadd213ps 576(%rdx), %zmm15, %zmm7
    vfmadd213ps 640(%rdx), %zmm8, %zmm0
    vfmadd213ps 640(%rdx), %zmm9, %zmm1
    vfmadd213ps 640(%rdx), %zmm10, %zmm2
    vfmadd213ps 640(%rdx), %zmm11, %zmm3
    vfmadd213ps 640(%rdx), %zmm12, %zmm4
    vfmadd213ps 640(%rdx), %zmm13, %zmm5
    vfmadd213ps 640(%rdx), %zmm14, %zmm6
    vfmadd213ps 640(%rdx), %zmm15, %zmm7
    vfmadd213ps 704(%rdx), %zmm8, %zmm0
    vfmadd213ps 704(%rdx), %zmm9, %zmm1
    vfmadd213ps 704(%rdx), %zmm10, %zmm2
    vfmadd213ps 704(%rdx), %zmm11, %zmm3
    vfmadd213ps 704(%rdx), %zmm12, %zmm4
    vfmadd213ps 704(%rdx), %zmm13, %zmm5
    vfmadd213ps 704(%rdx), %zmm14, %zmm6
    vfmadd213ps 704(%rdx), %zmm15, %zmm7
    vfmadd213ps 768(%rdx), %zmm8, %zmm0
    vfmadd213ps 768(%rdx), %zmm9, %zmm1
    vfmadd213ps 768(%rdx), %zmm10, %zmm2
    vfmadd213ps 768(%rdx), %zmm11, %zmm3
    vfmadd213ps 768(%rdx), %zmm12, %zmm4
    vfmadd213ps 768(%rdx), %zmm13, %zmm5
    vfmadd213ps 768(%rdx), %zmm14, %zmm6
    vfmadd213ps 768(%rdx), %zmm15, %zmm7
    vfmadd213ps 832(%rdx), %zmm8, %zmm0
    vfmadd213ps 832(%rdx), %zmm9, %zmm1
    vfmadd213ps 832(%rdx), %zmm10, %zmm2
    vfmadd213ps 832(%rdx), %zmm11, %zmm3
    vfmadd213ps 832(%rdx), %zmm12, %zmm4
    vfmadd213ps 832(%rdx), %zmm13, %zmm5
    vfmadd213ps 832(%rdx), %zmm14, %zmm6
    vfmadd213ps 832(%rdx), %zmm15, %zmm7
    vfmadd213ps 896(%rdx), %zmm8, %zmm0
    vfmadd213ps 896(%rdx), %zmm9, %zmm1
    vfmadd213ps 896(%rdx), %zmm10, %zmm2
    vfmadd213ps 896(%rdx), %zmm11, %zmm3
    vfmadd213ps 896(%rdx), %zmm12, %zmm4
    vfmadd213ps 896(%rdx), %zmm13, %zmm5
    vfmadd213ps 896(%rdx), %zmm14, %zmm6
    vfmadd213ps 896(%rdx), %zmm15, %zmm7
    vfmadd213ps 960(%rdx), %zmm8, %zmm0
    vfmadd213ps 960(%rdx), %zmm9, %zmm1
    vfmadd213ps 960(%rdx), %zmm10, %zmm2
    vfmadd213ps 960(%rdx), %zmm11, %zmm3
    vfmadd213ps 960(%rdx), %zmm12, %zmm4
    vfmadd213ps 960(%rdx), %zmm13, %zmm5
    vfmadd213ps 960(%rdx), %zmm14, %zmm6
    vfmadd213ps 960(%rdx), %zmm15, %zmm7
    vfmadd213ps 1024(%rdx), %zmm8, %zmm0
    vfmadd213ps 1024(%rdx), %zmm9, %zmm1
    vfmadd213ps 1024(%rdx), %zmm10, %zmm2
    vfmadd213ps 1024(%rdx), %zmm11, %zmm3
    vfmadd213ps 1024(%rdx), %zmm12, %zmm4
    vfmadd213ps 1024(%rdx), %zmm13, %zmm5
    vfmadd213ps 1024(%rdx), %zmm14, %zmm6
    vfmadd213ps 1024(%rdx), %zmm15, %zmm7
    vfmadd213ps 1088(%rdx), %zmm8, %zmm0
    vfmadd213ps 1088(%rdx), %zmm9, %zmm1
    vfmadd213ps 1088(%rdx), %zmm10, %zmm2
    vfmadd213ps 1088(%rdx), %zmm11, %zmm3
    vfmadd213ps 1088(%rdx), %zmm12, %zmm4
    vfmadd213ps 1088(%rdx), %zmm13, %zmm5
    vfmadd213ps 1088(%rdx), %zmm14, %zmm6
    vfmadd213ps 1088(%rdx), %zmm15, %zmm7
    vfmadd213ps 1152(%rdx), %zmm8, %zmm0
    vfmadd213ps 1152(%rdx), %zmm9, %zmm1
    vfmadd213ps 1152(%rdx), %zmm10, %zmm2
    vfmadd213ps 1152(%rdx), %zmm11, %zmm3
    vfmadd213ps 1152(%rdx), %zmm12, %zmm4
    vfmadd213ps 1152(%rdx), %zmm13, %zmm5
    vfmadd213ps 1152(%rdx), %zmm14, %zmm6
    vfmadd213ps 1152(%rdx), %zmm15, %zmm7
    vfmadd213ps 1216(%rdx), %zmm8, %zmm0
    vfmadd213ps 1216(%rdx), %zmm9, %zmm1
    vfmadd213ps 1216(%rdx), %zmm10, %zmm2
    vfmadd213ps 1216(%rdx), %zmm11, %zmm3
    vfmadd213ps 1216(%rdx), %zmm12, %zmm4
    vfmadd213ps 1216(%rdx), %zmm13, %zmm5
    vfmadd213ps 1216(%rdx), %zmm14, %zmm6
    vfmadd213ps 1216(%rdx), %zmm15, %zmm7
    vfmadd213ps 1280(%rdx), %zmm8, %zmm0
    vfmadd213ps 1280(%rdx), %zmm9, %zmm1
    vfmadd213ps 1280(%rdx), %zmm10, %zmm2
    vfmadd213ps 1280(%rdx), %zmm11, %zmm3
    vfmadd213ps 1280(%rdx), %zmm12, %zmm4
    vfmadd213ps 1280(%rdx), %zmm13, %zmm5
    vfmadd213ps 1280(%rdx), %zmm14, %zmm6
    vfmadd213ps 1280(%rdx), %zmm15, %zmm7
    vfmadd213ps 1344(%rdx), %zmm8, %zmm0
    vfmadd213ps 1344(%rdx), %zmm9, %zmm1
    vfmadd213ps 1344(%rdx), %zmm10, %zmm2
    vfmadd213ps 1344(%rdx), %zmm11, %zmm3
    vfmadd213ps 1344(%rdx), %zmm12, %zmm4
    vfmadd213ps 1344(%rdx), %zmm13, %zmm5
    vfmadd213ps 1344(%rdx), %zmm14, %zmm6
    vfmadd213ps 1344(%rdx), %zmm15, %zmm7
    vfmadd213ps 1408(%rdx), %zmm8, %zmm0
    vfmadd213ps 1408(%rdx), %zmm9, %zmm1
    vfmadd213ps 1408(%rdx), %zmm10, %zmm2
    vfmadd213ps 1408(%rdx), %zmm11, %zmm3
    vfmadd213ps 1408(%rdx), %zmm12, %zmm4
    vfmadd213ps 1408(%rdx), %zmm13, %zmm5
    vfmadd213ps 1408(%rdx), %zmm14, %zmm6
    vfmadd213ps 1408(%rdx), %zmm15, %zmm7
    vfmadd213ps 1472(%rdx), %zmm8, %zmm0
    vfmadd213ps 1472(%rdx), %zmm9, %zmm1
    vfmadd213ps 1472(%rdx), %zmm10, %zmm2
    vfmadd213ps 1472(%rdx), %zmm11, %zmm3
    vfmadd213ps 1472(%rdx), %zmm12, %zmm4
    vfmadd213ps 1472(%rdx), %zmm13, %zmm5
    vfmadd213ps 1472(%rdx), %zmm14, %zmm6
    vfmadd213ps 1472(%rdx), %zmm15, %zmm7
    vfmadd213ps 1536(%rdx), %zmm8, %zmm0
    vfmadd213ps 1536(%rdx), %zmm9, %zmm1
    vfmadd213ps 1536(%rdx), %zmm10, %zmm2
    vfmadd213ps 1536(%rdx), %zmm11, %zmm3
    vfmadd213ps 1536(%rdx), %zmm12, %zmm4
    vfmadd213ps 1536(%rdx), %zmm13, %zmm5
    vfmadd213ps 1536(%rdx), %zmm14, %zmm6
    vfmadd213ps 1536(%rdx), %zmm15, %zmm7
    vfmadd213ps 1600(%rdx), %zmm8, %zmm0
    vfmadd213ps 1600(%rdx), %zmm9, %zmm1
    vfmadd213ps 1600(%rdx), %zmm10, %zmm2
    vfmadd213ps 1600(%rdx), %zmm11, %zmm3
    vfmadd213ps 1600(%rdx), %zmm12, %zmm4
    vfmadd213ps 1600(%rdx), %zmm13, %zmm5
    vfmadd213ps 1600(%rdx), %zmm14, %zmm6
    vfmadd213ps 1600(%rdx), %zmm15, %zmm7
    vfmadd213ps 1664(%rdx), %zmm8, %zmm0
    vfmadd213ps 1664(%rdx), %zmm9, %zmm1
    vfmadd213ps 1664(%rdx), %zmm10, %zmm2
    vfmadd213ps 1664(%rdx), %zmm11, %zmm3
    vfmadd213ps 1664(%rdx), %zmm12, %zmm4
    vfmadd213ps 1664(%rdx), %zmm13, %zmm5
    vfmadd213ps 1664(%rdx), %zmm14, %zmm6
    vfmadd213ps 1664(%rdx), %zmm15, %zmm7
    vfmadd213ps 1728(%rdx), %zmm8, %zmm0
    vfmadd213ps 1728(%rdx), %zmm9, %zmm1
    vfmadd213ps 1728(%rdx), %zmm10, %zmm2
    vfmadd213ps 1728(%rdx), %zmm11, %zmm3
    vfmadd213ps 1728(%rdx), %zmm12, %zmm4
    vfmadd213ps 1728(%rdx), %zmm13, %zmm5
    vfmadd213ps 1728(%rdx), %zmm14, %zmm6
    vfmadd213ps 1728(%rdx), %zmm15, %zmm7
    vfmadd213ps 1792(%rdx), %zmm8, %zmm0
    vfmadd213ps 1792(%rdx), %zmm9, %zmm1
    vfmadd213ps 1792(%rdx), %zmm10, %zmm2
    vfmadd213ps 1792(%rdx), %zmm11, %zmm3
    vfmadd213ps 1792(%rdx), %zmm12, %zmm4
    vfmadd213ps 1792(%rdx), %zmm13, %zmm5
    vfmadd213ps 1792(%rdx), %zmm14, %zmm6
    vfmadd213ps 1792(%rdx), %zmm15, %zmm7
    vfmadd213ps 1856(%rdx), %zmm8, %zmm0
    vfmadd213ps 1856(%rdx), %zmm9, %zmm1
    vfmadd213ps 1856(%rdx), %zmm10, %zmm2
    vfmadd213ps 1856(%rdx), %zmm11, %zmm3
    vfmadd213ps 1856(%rdx), %zmm12, %zmm4
    vfmadd213ps 1856(%rdx), %zmm13, %zmm5
    vfmadd213ps 1856(%rdx), %zmm14, %zmm6
    vfmadd213ps 1856(%rdx), %zmm15, %zmm7
    vfmadd213ps 1920(%rdx), %zmm8, %zmm0
    vfmadd213ps 1920(%rdx), %zmm9, %zmm1
    vfmadd213ps 1920(%rdx), %zmm10, %zmm2
    vfmadd213ps 1920(%rdx), %zmm11, %zmm3
    vfmadd213ps 1920(%rdx), %zmm12, %zmm4
    vfmadd213ps 1920(%rdx), %zmm13, %zmm5
    vfmadd213ps 1920(%rdx), %zmm14, %zmm6
    vfmadd213ps 1920(%rdx), %zmm15, %zmm7
    vmovaps %zmm0, 0(%rdi)
    vmovaps %zmm1, 64(%rdi)
    vmovaps %zmm2, 128(%rdi)
    vmovaps %zmm3, 192(%rdi)
    vmovaps %zmm4, 256(%rdi)
    vmovaps %zmm5, 320(%rdi)
    vmovaps %zmm6, 384(%rdi)
    vmovaps %zmm7, 448(%rdi)
    add $512, %rsi
    add $512, %rdi
    sub $128, %rcx
    jg 1b
2:
    ret
