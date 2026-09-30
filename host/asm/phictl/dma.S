# dma.S: the SBOX DMA engine driven from the host
# (host/crates/phi-hw/src/dma.rs): one channel taken as host owned, its
# 256-descriptor ring in pinned host memory, copies submitted one
# 64-byte line at a time (the copy, a status descriptor that writes the
# sequence number after it, two NOPs), completion read from the status
# words rather than the tail pointer (which runs ahead of the writes,
# measured 2026-09-16). See dma.md.
	.include "defs.inc"

	.set RING_DESCRIPTORS, 256
	.set MAX_IN_FLIGHT, RING_DESCRIPTORS / 4 - 1
	.set COPY_TIMEOUT_MS, 2000

# A channel: 0 number, 8 ring host pointer, 16 ring card address, 24
# status slot card address, 32 head, 40 submitted, 48 pending_to_card,
# 56 completed, 64 progress ms, 72 in use.
	.set DC_CHAN, 0
	.set DC_RING, 8
	.set DC_RING_CARD, 16
	.set DC_STATUS_CARD, 24
	.set DC_HEAD, 32
	.set DC_SUBMITTED, 40
	.set DC_PENDING, 48
	.set DC_COMPLETED, 56
	.set DC_PROGRESS, 64
	.set DC_OPEN, 72
	.set DC_SIZE, 80
	.globl DC_CHAN, DC_RING, DC_RING_CARD, DC_STATUS_CARD, DC_HEAD, DC_SUBMITTED, DC_PENDING, DC_COMPLETED, DC_PROGRESS, DC_OPEN, DC_SIZE

	.text

	.globl dma_buffer
# dma_buffer(rdi = iova, rsi = len) -> rax = a populated private
# anonymous mapping of len bytes pinned for the card at iova, or -errno.
# The card address is SMPT_BASE + iova (the identity SMPT map).
dma_buffer:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rdi, %r12
	movq %rsi, %r13
	xorl %edi, %edi
	movq %r13, %rsi
	movl $PROT_READ | PROT_WRITE, %edx
	movl $MAP_PRIVATE | MAP_ANONYMOUS | MAP_POPULATE, %r10d
	movq $-1, %r8
	xorl %r9d, %r9d
	SYS SYS_mmap
	cmpq $-4096, %rax
	ja 9f
	movq %rax, %rbx
	movq %rbx, %rdi
	movq %r12, %rsi
	movq %r13, %rdx
	call vfio_map_dma
	testq %rax, %rax
	js 8f
	movq %rbx, %rax
	jmp 9f
8:	pushq %rax
	movq %rbx, %rdi
	movq %r13, %rsi
	SYS SYS_munmap
	popq %rax
9:	popq %r13
	popq %r12
	popq %rbx
	ret

	.globl card_addr_of
# card_addr_of(rdi = iova) -> rax = SMPT_BASE + iova.
card_addr_of:
	movabsq $SMPT_BASE_HI << 32, %rax
	addq %rdi, %rax
	ret

	.globl dma_open
# dma_open(rdi = channel struct, esi = channel number, rdx = ring iova,
# rcx = status slot card address) -> eax 1 when the channel is up: 8 KiB
# pinned (the ring in the first page, status words in the second), the
# card's status word cleared (a previous daemon left its last sequence
# number there), the channel taken for the host and enabled, its head
# aligned to a line. Failures are logged and give 0.
dma_open:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq %rdi, %rbx
	movl %esi, DC_CHAN(%rbx)
	movq %rcx, DC_STATUS_CARD(%rbx)
	movq $0, DC_OPEN(%rbx)
	testq $63, %rcx
	jnz 90f
	cmpl $8, %esi
	jae 90f
	movq %rdx, %r12
	movl $4, %edi
	call smpt_identity
	movq %r12, %rdi
	movl $8192, %esi
	call dma_buffer
	testq %rax, %rax
	js 91f
	movq %rax, DC_RING(%rbx)
	movq %r12, %rdi
	call card_addr_of
	movq %rax, DC_RING_CARD(%rbx)
	movq %rax, %r13
	# the card-side status word cleared
	movq DC_STATUS_CARD(%rbx), %rdi
	leaq zero8(%rip), %rsi
	movl $8, %edx
	call write_card_memory
	# DCR: owner host, disabled while the ring is set
	movl DC_CHAN(%rbx), %ecx
	addl %ecx, %ecx
	movl $3, %r14d
	shll %cl, %r14d		# the channel's two bits
	movl $DCR, %edi
	call sbox_read
	notl %r14d
	andl %r14d, %eax
	notl %r14d
	movl %eax, %r15d	# dcr without the channel's bits
	movl DC_CHAN(%rbx), %ecx
	addl %ecx, %ecx
	movl $1, %esi
	shll %cl, %esi
	orl %r15d, %esi
	movl $DCR, %edi
	call sbox_write
	# DRAR: the ring's card address, size, SMPT page, SYS
	movl $DRAR_LO, %esi
	call dma_reg
	movl %eax, %edi
	movl %r13d, %esi
	call sbox_write
	movl $RING_DESCRIPTORS, %esi
	shll $4, %esi
	movq %r13, %rax
	shrq $32, %rax
	andl $3, %eax
	orl %eax, %esi
	movq %r13, %rax
	shrq $SMPT_PAGE_SHIFT, %rax
	andl $0x1f, %eax
	shll $21, %eax
	orl %eax, %esi
	orl $DRAR_HI_SYS, %esi
	pushq %rsi
	movl $DRAR_HI, %esi
	call dma_reg
	popq %rsi
	movl %eax, %edi
	call sbox_write
	# DCAR: both interrupts masked
	movl $DCAR, %esi
	call dma_reg
	movl %eax, %r12d
	movl %eax, %edi
	call sbox_read
	orl $DCAR_IM0 | DCAR_IM1, %eax
	movl %r12d, %edi
	movl %eax, %esi
	call sbox_write
	movl $DCHERRMSK, %esi
	call dma_reg
	movl %eax, %edi
	xorl %esi, %esi
	call sbox_write
	# head on a line boundary: a fresh channel reports tail 0; anything
	# else is realigned upwards with NOPs
	movl $DTPR, %esi
	call dma_reg
	movl %eax, %edi
	call sbox_read
	andl $RING_DESCRIPTORS - 1, %eax
	movl %eax, %r12d	# tail
	addl $3, %eax
	andl $-4, %eax
	andl $RING_DESCRIPTORS - 1, %eax
	movl %eax, DC_HEAD(%rbx)
	cmpl %r12d, %eax
	je 2f
	movq DC_RING(%rbx), %rdi
	movl %r12d, %ecx
1:	cmpl DC_HEAD(%rbx), %ecx
	je 2f
	movl %ecx, %eax
	shll $4, %eax
	movq $0, (%rdi,%rax)
	movq $0, 8(%rdi,%rax)
	incl %ecx
	andl $RING_DESCRIPTORS - 1, %ecx
	jmp 1b
2:	mfence
	movl $DHPR, %esi
	call dma_reg
	movl %eax, %edi
	movl DC_HEAD(%rbx), %esi
	call sbox_write
	movl DC_CHAN(%rbx), %ecx
	addl %ecx, %ecx
	movl $3, %esi
	shll %cl, %esi
	orl %r15d, %esi
	movl $DCR, %edi
	call sbox_write
	movq $0, DC_SUBMITTED(%rbx)
	movq $0, DC_PENDING(%rbx)
	movq $0, DC_COMPLETED(%rbx)
	call now_ms
	movq %rax, DC_PROGRESS(%rbx)
	movq $1, DC_OPEN(%rbx)
	# log
	call l_begin
	call l_tag
	leaq s_dma1(%rip), %rsi
	call w_cstr
	movl DC_CHAN(%rbx), %edi
	call w_dec
	leaq s_dma2(%rip), %rsi
	call w_cstr
	movq DC_RING_CARD(%rbx), %rdi
	call w_hex
	leaq s_dma3(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	call w_dec
	leaq s_dma4(%rip), %rsi
	call w_cstr
	movl $DCR, %edi
	call sbox_read
	movl %eax, %edi
	call w_hex
	call l_end
	# realigned: wait until the engine consumed the NOPs
	movl DC_HEAD(%rbx), %eax
	cmpl %r12d, %eax
	je 3f
	movq %rbx, %rdi
	call dma_wait_idle
	testl %eax, %eax
	jz 92f
3:	movl $1, %eax
	jmp 99f
90:	leaq s_e_dma_arg(%rip), %rsi
	jmp 95f
91:	leaq s_e_dma_buf(%rip), %rsi
	jmp 95f
92:	leaq s_e_dma_drain(%rip), %rsi
95:	pushq %rsi
	call l_begin
	call l_tag
	leaq s_dma_unusable(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	leaq s_dma_aper(%rip), %rsi
	call w_cstr
	call l_end
	xorl %eax, %eax
99:	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
	.section .rodata
	.p2align 3
zero8:	.quad 0
	.text
STR s_dma1, "DMA channel "
STR s_dma2, ": ring at card "
STR s_dma3, " (256 descriptors), tail "
STR s_dma4, ", DCR "
STR s_dma_unusable, "disk: DMA channel unusable ("
STR s_dma_aper, "); copying through the aperture"
STR s_e_dma_arg, "bad channel or status slot"
STR s_e_dma_buf, "the descriptor ring could not be pinned"
STR s_e_dma_drain, "the channel did not drain its realignment"

# dma_reg(rbx = channel struct, esi = register) -> eax = SBOX offset.
dma_reg:
	movl DC_CHAN(%rbx), %eax
	imull $DMA_CHAN_STRIDE, %eax, %eax
	addl $DMA_CHAN_BASE, %eax
	addl %esi, %eax
	ret

# dma_wait_idle(rdi = channel) -> eax 1 when the tail reached the head
# within the timeout.
dma_wait_idle:
	pushq %rbx
	pushq %r12
	movq %rdi, %rbx
	call now_ms
	addq $COPY_TIMEOUT_MS, %rax
	movq %rax, %r12
1:	movl $DTPR, %esi
	call dma_reg
	movl %eax, %edi
	call sbox_read
	andl $RING_DESCRIPTORS - 1, %eax
	cmpl DC_HEAD(%rbx), %eax
	je 2f
	call now_ms
	cmpq %r12, %rax
	jae 3f
	pause
	jmp 1b
2:	movl $1, %eax
	popq %r12
	popq %rbx
	ret
3:	xorl %eax, %eax
	popq %r12
	popq %rbx
	ret

	.globl dma_in_flight
# dma_in_flight(rdi = channel) -> rax.
dma_in_flight:
	movq DC_SUBMITTED(%rdi), %rax
	subq DC_COMPLETED(%rdi), %rax
	ret

	.globl dma_submit
# dma_submit(rdi = channel, rsi = src card address, rdx = dst card
# address, rcx = len) -> rax = the copy's sequence number, or 0 when it
# failed (logged): addresses and length multiples of 64, the length under
# 1 MiB. Blocks only while the ring is full.
dma_submit:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq %rdi, %rbx
	movq %rsi, %r12
	movq %rdx, %r13
	movq %rcx, %r14
	testq %r14, %r14
	jz 90f
	movq %r14, %rax
	orq %r12, %rax
	orq %r13, %rax
	testq $63, %rax
	jnz 90f
	cmpq $1 << 20, %r14
	jae 90f
1:	movq %rbx, %rdi
	call dma_in_flight
	cmpq $MAX_IN_FLIGHT, %rax
	jb 2f
	movq %rbx, %rdi
	call dma_poll
	testq %rax, %rax
	js 91f
	pause
	jmp 1b
2:	movq DC_SUBMITTED(%rbx), %r15
	incq %r15		# seq
	# the status word: in host memory for copies into host memory, in
	# card memory for copies into the card
	movabsq $SMPT_BASE_HI << 32, %rax
	cmpq %rax, %r13
	jb 3f
	movq DC_RING_CARD(%rbx), %r9
	addq $4096, %r9
	jmp 4f
3:	movq DC_STATUS_CARD(%rbx), %r9
4:	# descriptors at head * 16 in the ring
	movl DC_HEAD(%rbx), %eax
	shll $4, %eax
	movq DC_RING(%rbx), %rdi
	addq %rax, %rdi
	# memcpy: q0 = src[39:0] | (len/64) << 46; q1 = dst[39:0] | 1 << 60
	movabsq $(1 << 40) - 1, %r8
	movq %r12, %rax
	andq %r8, %rax
	movq %r14, %rcx
	shrq $6, %rcx
	shlq $46, %rcx
	orq %rcx, %rax
	movq %rax, 0(%rdi)
	movq %r13, %rax
	andq %r8, %rax
	movabsq $1 << 60, %rcx
	orq %rcx, %rax
	movq %rax, 8(%rdi)
	# status: q0 = seq; q1 = dst[39:0] | 2 << 60
	movq %r15, 16(%rdi)
	movq %r9, %rax
	andq %r8, %rax
	movabsq $2 << 60, %rcx
	orq %rcx, %rax
	movq %rax, 24(%rdi)
	movq $0, 32(%rdi)
	movq $0, 40(%rdi)
	movq $0, 48(%rdi)
	movq $0, 56(%rdi)
	mfence
	movl DC_HEAD(%rbx), %eax
	addl $4, %eax
	andl $RING_DESCRIPTORS - 1, %eax
	movl %eax, DC_HEAD(%rbx)
	movl $DHPR, %esi
	call dma_reg
	movl %eax, %edi
	movl DC_HEAD(%rbx), %esi
	call sbox_write
	movq %rbx, %rdi
	call dma_in_flight
	testq %rax, %rax
	jnz 5f
	call now_ms
	movq %rax, DC_PROGRESS(%rbx)
5:	movq %r15, DC_SUBMITTED(%rbx)
	movabsq $SMPT_BASE_HI << 32, %rax
	cmpq %rax, %r13
	jae 6f
	movq %r15, DC_PENDING(%rbx)
	cmpq %rax, %r12
	jb 7f
	addq %r14, traffic(%rip)
	incq traffic+32(%rip)
	jmp 7f
6:	addq %r14, traffic+8(%rip)
	incq traffic+40(%rip)
7:	movq %r15, %rax
	jmp 99f
90:	call l_begin
	call l_tag
	leaq s_e_granular(%rip), %rsi
	call w_cstr
	call l_end
91:	xorl %eax, %eax
99:	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_e_granular, "disk: DMA copy not 64-byte granular or over 1 MiB"

	.globl dma_poll
# dma_poll(rdi = channel) -> rax = the last completed sequence number,
# or -1 when a copy has been outstanding for COPY_TIMEOUT_MS or the
# status word is ahead of what was submitted (logged with the channel's
# registers).
dma_poll:
	pushq %rbx
	pushq %r12
	movq %rdi, %rbx
	call dma_in_flight
	testq %rax, %rax
	jnz 1f
	call now_ms
	movq %rax, DC_PROGRESS(%rbx)
	movq DC_COMPLETED(%rbx), %rax
	jmp 9f
1:	movq DC_RING(%rbx), %rax
	movq 4096(%rax), %r12		# the host status word
	movq DC_PENDING(%rbx), %rax
	cmpq %r12, %rax
	jbe 2f
	movq DC_STATUS_CARD(%rbx), %rdi
	call aper_read64
	cmpq %r12, %rax
	jbe 2f
	movq %rax, %r12
2:	cmpq DC_SUBMITTED(%rbx), %r12
	ja 8f
	cmpq DC_COMPLETED(%rbx), %r12
	jbe 3f
	movq %r12, DC_COMPLETED(%rbx)
	call now_ms
	movq %rax, DC_PROGRESS(%rbx)
	movq DC_PENDING(%rbx), %rax
	cmpq %r12, %rax
	ja 4f
	movq $0, DC_PENDING(%rbx)
	jmp 4f
3:	call now_ms
	subq DC_PROGRESS(%rbx), %rax
	cmpq $COPY_TIMEOUT_MS, %rax
	jbe 4f
	leaq s_e_timeout(%rip), %rsi
	jmp 7f
4:	movq DC_COMPLETED(%rbx), %rax
	jmp 9f
8:	leaq s_e_ahead(%rip), %rsi
7:	pushq %rsi
	call l_begin
	call l_tag
	leaq s_dma_pre(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	leaq s_dma_ch(%rip), %rsi
	call w_cstr
	movl DC_CHAN(%rbx), %edi
	call w_dec
	leaq s_dma_head(%rip), %rsi
	call w_cstr
	movl DC_HEAD(%rbx), %edi
	call w_dec
	leaq s_dma_tail(%rip), %rsi
	call w_cstr
	movl $DTPR, %esi
	call dma_reg
	movl %eax, %edi
	call sbox_read
	andl $RING_DESCRIPTORS - 1, %eax
	movl %eax, %edi
	call w_dec
	leaq s_dma_done(%rip), %rsi
	call w_cstr
	movq DC_COMPLETED(%rbx), %rdi
	call w_dec
	leaq s_dma_of(%rip), %rsi
	call w_cstr
	movq DC_SUBMITTED(%rbx), %rdi
	call w_dec
	leaq s_dma_err(%rip), %rsi
	call w_cstr
	movl $DCHERR, %esi
	call dma_reg
	movl %eax, %edi
	call sbox_read
	movl %eax, %edi
	call w_hex
	leaq s_dma_stat(%rip), %rsi
	call w_cstr
	movl $DSTAT, %esi
	call dma_reg
	movl %eax, %edi
	call sbox_read
	movl %eax, %edi
	call w_hex
	call l_end
	movq $-1, %rax
9:	popq %r12
	popq %rbx
	ret
STR s_dma_pre, "disk: DMA engine: "
STR s_e_timeout, "copy timed out"
STR s_e_ahead, "status word is ahead of the copies submitted"
STR s_dma_ch, "; channel "
STR s_dma_head, ": head "
STR s_dma_tail, " tail "
STR s_dma_done, ", completed "
STR s_dma_of, " of "
STR s_dma_err, " submitted, DCHERR "
STR s_dma_stat, ", DSTAT "

	.globl dma_wait
# dma_wait(rdi = channel, rsi = seq) -> eax 1 when the copy completed,
# 0 on failure.
dma_wait:
	pushq %rbx
	pushq %r12
	movq %rdi, %rbx
	movq %rsi, %r12
1:	movq %rbx, %rdi
	call dma_poll
	testq %rax, %rax
	js 8f
	cmpq %r12, %rax
	jae 9f
	pause
	jmp 1b
8:	xorl %eax, %eax
	popq %r12
	popq %rbx
	ret
9:	movl $1, %eax
	popq %r12
	popq %rbx
	ret

	.globl dma_copy
# dma_copy(rdi = channel, rsi = src, rdx = dst, rcx = len) -> eax 1 when
# the data is at the destination.
dma_copy:
	pushq %rdi
	call dma_submit
	popq %rdi
	testq %rax, %rax
	jz 1f
	movq %rax, %rsi
	jmp dma_wait
1:	xorl %eax, %eax
	ret

	.globl dma_close
# dma_close(rdi = channel): the channel disabled (its owner bit kept).
dma_close:
	pushq %rbx
	movq %rdi, %rbx
	cmpq $0, DC_OPEN(%rbx)
	je 9f
	movl $DCR, %edi
	call sbox_read
	movl DC_CHAN(%rbx), %ecx
	addl %ecx, %ecx
	movl $2, %esi
	shll %cl, %esi
	notl %esi
	andl %eax, %esi
	movl $DCR, %edi
	call sbox_write
	movq $0, DC_OPEN(%rbx)
9:	popq %rbx
	ret
