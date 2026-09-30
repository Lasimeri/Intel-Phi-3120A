# ring.S: the ring transport region in card memory, reached through the
# aperture (host/crates/phi-ring and phi-hw's ringmem.rs): the region
# header and channel table, the single-producer single-consumer byte
# rings, formatting a region in host memory, opening one on the card.
# Offsets are region-relative; region_base holds the card address. See
# ring.md.
	.include "defs.inc"

	.bss
	.p2align 3
	.globl region_base, region_size
region_base:	.skip 8
region_size:	.skip 8
# The channels found by region_open: 32-byte entries (kind, h2c offset,
# c2h offset, data offset, data size), and how many.
	.globl channels, nchannels
nchannels:	.skip 8
channels:	.skip 8 * 40
# Where a format writes: host memory (a buffer) instead of the aperture.
fmt_buf:	.skip 8

	.text

# ------------------------------------------------------------ memory

# The region's memory: aperture accesses at region_base + offset. Ring
# indices are single 32-bit accesses (a byte-wise index publish was
# read torn by the card, 2026-09-16); a fence is one read of the region,
# which drains this CPU's posted writes to the device before an index is
# published.

	.globl rm_read32
rm_read32:
	addq region_base(%rip), %rdi
	jmp aper_read32

	.globl rm_write32
rm_write32:
	addq region_base(%rip), %rdi
	jmp aper_write32

	.globl rm_read64
rm_read64:
	addq region_base(%rip), %rdi
	jmp aper_read64

	.globl rm_write64
rm_write64:
	addq region_base(%rip), %rdi
	jmp aper_write64

	.globl rm_read
# rm_read(rdi = offset, rsi = buf, rdx = len).
rm_read:
	addq region_base(%rip), %rdi
	jmp aper_read

	.globl rm_write
# rm_write(rdi = offset, rsi = data, rdx = len).
rm_write:
	addq region_base(%rip), %rdi
	jmp aper_write

	.globl rm_fence
rm_fence:
	xorl %edi, %edi
	jmp rm_read32

# ------------------------------------------------------------ rings

	.globl ring_size_of
# ring_size_of(rdi = ring offset) -> eax = the data size, edx = 1; edx = 0
# when the magic or the size is wrong.
ring_size_of:
	pushq %rdi
	call rm_read32
	popq %rdi
	cmpl $RING_MAGIC, %eax
	jne 8f
	addq $RG_SIZE, %rdi
	call rm_read32
	testl %eax, %eax
	jz 8f
	leal -1(%rax), %ecx
	testl %ecx, %eax
	jnz 8f
	movl $1, %edx
	ret
8:	xorl %edx, %edx
	ret

	.globl ring_free
# ring_free(rdi = ring offset, esi = size) -> eax: bytes a producer can
# push (a tail ahead of the head reads as full).
ring_free:
	pushq %rbx
	pushq %r12
	movq %rdi, %rbx
	movl %esi, %r12d
	leaq RG_HEAD(%rbx), %rdi
	call rm_read32
	movl %eax, %ecx
	leaq RG_TAIL(%rbx), %rdi
	pushq %rcx
	call rm_read32
	popq %rcx
	subl %eax, %ecx		# head - tail
	movl %r12d, %eax
	subl %ecx, %eax
	jae 1f
	xorl %eax, %eax
1:	popq %r12
	popq %rbx
	ret

	.globl ring_avail
# ring_avail(rdi = ring offset, esi = size) -> eax: bytes a consumer can
# pop; 0 when the indices are inconsistent (head - tail > size).
ring_avail:
	pushq %rbx
	pushq %r12
	movq %rdi, %rbx
	movl %esi, %r12d
	leaq RG_HEAD(%rbx), %rdi
	call rm_read32
	movl %eax, %ecx
	leaq RG_TAIL(%rbx), %rdi
	pushq %rcx
	call rm_read32
	popq %rcx
	subl %eax, %ecx
	movl %ecx, %eax
	cmpl %r12d, %ecx
	jbe 1f
	xorl %eax, %eax
1:	popq %r12
	popq %rbx
	ret

	.globl ring_push
# ring_push(rdi = ring offset, esi = size, rdx = data, rcx = len) ->
# rax = bytes pushed: what fits, written in one or two pieces, fenced,
# then head published in one 32-bit write.
ring_push:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq %rdi, %rbx
	movl %esi, %r12d
	movq %rdx, %r13
	movq %rcx, %r14
	leaq RG_HEAD(%rbx), %rdi
	call rm_read32
	movl %eax, %r15d	# head
	leaq RG_TAIL(%rbx), %rdi
	call rm_read32
	movl %r15d, %ecx
	subl %eax, %ecx		# used
	movl %r12d, %eax
	subl %ecx, %eax		# free
	jbe 9f
	cmpq %r14, %rax
	jbe 1f
	movq %r14, %rax
1:	movq %rax, %r14		# n
	testq %r14, %r14
	jz 9f
	leal -1(%r12), %eax
	andl %r15d, %eax	# start
	movl %r12d, %edx
	subl %eax, %edx		# room to the end
	cmpq %r14, %rdx
	jbe 2f
	movq %r14, %rdx
2:	pushq %rdx
	leaq RG_DATA(%rbx,%rax), %rdi
	movq %r13, %rsi
	call rm_write
	popq %rdx
	cmpq %r14, %rdx
	jae 3f
	leaq RG_DATA(%rbx), %rdi
	leaq (%r13,%rdx), %rsi
	movq %r14, %rcx
	subq %rdx, %rcx
	movq %rcx, %rdx
	call rm_write
3:	call rm_fence
	leaq RG_HEAD(%rbx), %rdi
	leal (%r15,%r14), %esi
	call rm_write32
	call rm_fence
	movq %r14, %rax
	jmp 10f
9:	xorl %eax, %eax
10:	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

	.globl ring_pop
# ring_pop(rdi = ring offset, esi = size, rdx = buf, rcx = cap) -> rax =
# bytes popped. Inconsistent indices are resolved by tail = head (the
# other side reset), dropping what was there.
ring_pop:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq %rdi, %rbx
	movl %esi, %r12d
	movq %rdx, %r13
	movq %rcx, %r14
	leaq RG_HEAD(%rbx), %rdi
	call rm_read32
	movl %eax, %r15d	# head
	leaq RG_TAIL(%rbx), %rdi
	call rm_read32
	movl %eax, %r10d	# tail
	movl %r15d, %ecx
	subl %r10d, %ecx	# avail
	cmpl %r12d, %ecx
	jbe 1f
	leaq RG_TAIL(%rbx), %rdi
	movl %r15d, %esi
	call rm_write32
	call rm_fence
	jmp 9f
1:	movl %ecx, %eax
	cmpq %r14, %rax
	jbe 2f
	movq %r14, %rax
2:	movq %rax, %r14		# n
	testq %r14, %r14
	jz 9f
	movl %r10d, %r15d	# tail (head no longer needed)
	leal -1(%r12), %eax
	andl %r10d, %eax	# start
	movl %r12d, %edx
	subl %eax, %edx
	cmpq %r14, %rdx
	jbe 3f
	movq %r14, %rdx
3:	pushq %rdx
	leaq RG_DATA(%rbx,%rax), %rdi
	movq %r13, %rsi
	call rm_read
	popq %rdx
	cmpq %r14, %rdx
	jae 4f
	leaq RG_DATA(%rbx), %rdi
	leaq (%r13,%rdx), %rsi
	movq %r14, %rcx
	subq %rdx, %rcx
	movq %rcx, %rdx
	call rm_read
4:	call rm_fence
	leaq RG_TAIL(%rbx), %rdi
	leal (%r15,%r14), %esi
	call rm_write32
	call rm_fence
	movq %r14, %rax
	jmp 10f
9:	xorl %eax, %eax
10:	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ the region

	.globl region_open
# region_open() -> eax 1 when the region at region_base holds a valid
# header, with channels/nchannels filled (unknown kinds skipped, every
# offset checked against the recorded size, as phi-ring's open).
region_open:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq $0, nchannels(%rip)
	movl $RH_MAGIC, %edi
	call rm_read32
	cmpl $REGION_MAGIC, %eax
	jne 90f
	movl $RH_VERSION, %edi
	call rm_read32
	cmpl $RING_VERSION, %eax
	jne 90f
	movl $RH_REGION_SIZE, %edi
	call rm_read32
	movl %eax, %r12d		# size
	cmpq region_size(%rip), %r12
	ja 90f
	movl $RH_CHANNEL_COUNT, %edi
	call rm_read32
	movl %eax, %r13d		# count
	movq %r13, %rax
	shlq $6, %rax
	addq $REGION_HDR_SIZE, %rax
	cmpq %r12, %rax
	ja 90f
	xorl %r14d, %r14d
1:	cmpq %r13, %r14
	jae 80f
	movq %r14, %rbx
	shlq $6, %rbx
	addq $REGION_HDR_SIZE, %rbx	# descriptor offset
	leaq CD_KIND(%rbx), %rdi
	call rm_read32
	movl %eax, %r15d
	cmpl $1, %r15d
	jb 7f
	cmpl $5, %r15d
	ja 7f
	# both rings: 64-byte aligned, a power-of-two size, inside the region
	leaq CD_H2C_OFFSET(%rbx), %rdi
	call rm_read32
	movl %eax, %edi
	leaq CD_H2C_SIZE(%rbx), %rsi
	pushq %rdi
	movq %rsi, %rdi
	call rm_read32
	popq %rdi
	movl %eax, %esi
	call ring_fits
	testl %eax, %eax
	jz 90f
	leaq CD_C2H_OFFSET(%rbx), %rdi
	call rm_read32
	movl %eax, %edi
	pushq %rdi
	leaq CD_C2H_SIZE(%rbx), %rdi
	call rm_read32
	popq %rdi
	movl %eax, %esi
	call ring_fits
	testl %eax, %eax
	jz 90f
	# the data area, when any
	leaq CD_DATA_OFFSET(%rbx), %rdi
	call rm_read32
	movl %eax, %edi
	pushq %rdi
	leaq CD_DATA_SIZE(%rbx), %rdi
	call rm_read32
	popq %rdi
	movl %edi, %ecx
	orl %eax, %ecx
	jz 5f
	testl $DATA_ALIGN - 1, %edi
	jnz 90f
	testl %eax, %eax
	jz 90f
	leaq (%rdi,%rax), %rcx
	cmpq %r12, %rcx
	ja 90f
5:	# record it
	movq nchannels(%rip), %rax
	shlq $5, %rax
	leaq channels(%rip), %rcx
	addq %rax, %rcx
	movq %r15, (%rcx)
	pushq %rcx
	leaq CD_H2C_OFFSET(%rbx), %rdi
	call rm_read32
	movq (%rsp), %rcx
	movq %rax, 8(%rcx)
	leaq CD_C2H_OFFSET(%rbx), %rdi
	call rm_read32
	movq (%rsp), %rcx
	movq %rax, 16(%rcx)
	leaq CD_DATA_OFFSET(%rbx), %rdi
	call rm_read32
	movq (%rsp), %rcx
	movq %rax, 24(%rcx)
	leaq CD_DATA_SIZE(%rbx), %rdi
	call rm_read32
	popq %rcx
	movl %eax, 28(%rcx)
	incq nchannels(%rip)
7:	incq %r14
	jmp 1b
80:	movl $1, %eax
	jmp 99f
90:	xorl %eax, %eax
99:	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# ring_fits(edi = ring offset, esi = data size; r12 = region size) ->
# eax 1 when aligned, a non-zero power of two, and inside the region.
ring_fits:
	testl $LINE - 1, %edi
	jnz 8f
	testl %esi, %esi
	jz 8f
	leal -1(%rsi), %eax
	testl %esi, %eax
	jnz 8f
	movl %edi, %eax
	addq $RING_HDR_SIZE, %rax
	addq %rsi, %rax
	cmpq %r12, %rax
	ja 8f
	movl $1, %eax
	ret
8:	xorl %eax, %eax
	ret

	.globl channel_of
# channel_of(edi = kind) -> rax = its channels entry, or 0.
channel_of:
	movq nchannels(%rip), %rcx
1:	testq %rcx, %rcx
	jz 9f
	decq %rcx
	movq %rcx, %rax
	shlq $5, %rax
	leaq channels(%rip), %rdx
	addq %rdx, %rax
	cmpl %edi, (%rax)
	jne 1b
	ret
9:	xorl %eax, %eax
	ret

	.globl endpoints_of
# endpoints_of(edi = kind) -> eax 1 with the host endpoints: r8 = the
# host-to-card ring offset, r9d = its size, r10 = the card-to-host ring
# offset, r11d = its size (a producer on h2c, a consumer on c2h).
endpoints_of:
	pushq %rbx
	call channel_of
	testq %rax, %rax
	jz 9f
	movq %rax, %rbx
	movq 8(%rbx), %rdi
	call ring_size_of
	testl %edx, %edx
	jz 9f
	movl %eax, %r9d
	movq 8(%rbx), %r8
	movq 16(%rbx), %rdi
	call ring_size_of
	testl %edx, %edx
	jz 9f
	movl %eax, %r11d
	movq 16(%rbx), %r10
	movl $1, %eax
	popq %rbx
	ret
9:	xorl %eax, %eax
	popq %rbx
	ret

	.globl card_boot_flags
card_boot_flags:
	movl $RH_CARD_BOOT_FLAGS, %edi
	jmp rm_read64

# ------------------------------------------------------------ formatting

# The default channel plan (phi-hw boot.rs DEFAULT_CHANNELS): kind, h2c
# size, c2h size, data size.
	.section .rodata
	.p2align 3
	.globl default_plan
default_plan:
	.long KIND_CONSOLE, 4096, 65536, 0
	.long KIND_NETWORK, 262144, 262144, 0
	.long KIND_RPC, 262144, 262144, 0
	.long KIND_BLOCK, 16384, 65536, 8 << 20
	.long KIND_HOSTMEM, 16384, 65536, 0
	.set NPLAN, 5
	.text

# fb32(rdi = offset, esi = value): a u32 into the format buffer.
fb32:
	movq fmt_buf(%rip), %rax
	movl %esi, (%rax,%rdi)
	ret
fb64:
	movq fmt_buf(%rip), %rax
	movq %rsi, (%rax,%rdi)
	ret

	.globl region_format
# region_format(rdi = buffer, rsi = region size, rdx = host epoch ns,
# rcx = host memory card address, r8 = host memory size): lay the default
# plan out in the buffer (rings after the table, each 64-byte aligned;
# data areas after the last ring, page aligned) and write the header
# with the magic last. Exits when the plan does not fit. Returns rax =
# the first byte past the layout.
region_format:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	subq $40, %rsp
	movq %rdi, fmt_buf(%rip)
	movq %rsi, %r12		# region size
	movq %rdx, 0(%rsp)
	movq %rcx, 8(%rsp)
	movq %r8, 16(%rsp)
	# zero the buffer
	movq %rdi, %rcx
	movq %rsi, %rdx
	pushq %rdi
	xorl %eax, %eax
	movq %rdx, %rcx
	rep stosb
	popq %rdi
	# rings
	movl $REGION_HDR_SIZE + NPLAN * CHANNEL_DESC_SIZE, %r13d	# cursor
	addq $LINE - 1, %r13
	andq $-LINE, %r13
	xorl %r14d, %r14d		# plan index
1:	cmpl $NPLAN, %r14d
	jae 3f
	leaq default_plan(%rip), %rbx
	movl %r14d, %eax
	shll $4, %eax
	addq %rax, %rbx
	# descriptor
	movl %r14d, %r15d
	shll $6, %r15d
	addl $REGION_HDR_SIZE, %r15d
	leaq CD_KIND(%r15), %rdi
	movl (%rbx), %esi
	call fb32
	leaq CD_H2C_OFFSET(%r15), %rdi
	movl %r13d, %esi
	call fb32
	leaq CD_H2C_SIZE(%r15), %rdi
	movl 4(%rbx), %esi
	call fb32
	movq %r13, %rdi
	movl 4(%rbx), %esi
	call format_ring
	movl 4(%rbx), %eax
	leaq RING_HDR_SIZE(%r13,%rax), %r13
	addq $LINE - 1, %r13
	andq $-LINE, %r13
	leaq CD_C2H_OFFSET(%r15), %rdi
	movl %r13d, %esi
	call fb32
	leaq CD_C2H_SIZE(%r15), %rdi
	movl 8(%rbx), %esi
	call fb32
	movq %r13, %rdi
	movl 8(%rbx), %esi
	call format_ring
	movl 8(%rbx), %eax
	leaq RING_HDR_SIZE(%r13,%rax), %r13
	addq $LINE - 1, %r13
	andq $-LINE, %r13
	incl %r14d
	jmp 1b
3:	# data areas
	xorl %r14d, %r14d
4:	cmpl $NPLAN, %r14d
	jae 6f
	leaq default_plan(%rip), %rbx
	movl %r14d, %eax
	shll $4, %eax
	addq %rax, %rbx
	movl 12(%rbx), %eax
	testl %eax, %eax
	jz 5f
	addq $DATA_ALIGN - 1, %r13
	andq $-DATA_ALIGN, %r13
	movl %r14d, %r15d
	shll $6, %r15d
	addl $REGION_HDR_SIZE, %r15d
	leaq CD_DATA_OFFSET(%r15), %rdi
	movl %r13d, %esi
	call fb32
	leaq CD_DATA_SIZE(%r15), %rdi
	movl 12(%rbx), %esi
	call fb32
	movl 12(%rbx), %eax
	addq %rax, %r13
	addq $DATA_ALIGN - 1, %r13
	andq $-DATA_ALIGN, %r13
5:	incl %r14d
	jmp 4b
6:	cmpq %r12, %r13
	ja 90f
	movl $RH_VERSION, %edi
	movl $RING_VERSION, %esi
	call fb32
	movl $RH_REGION_SIZE, %edi
	movl %r12d, %esi
	call fb32
	movl $RH_CHANNEL_COUNT, %edi
	movl $NPLAN, %esi
	call fb32
	movl $RH_HOST_EPOCH_NS, %edi
	movq 0(%rsp), %rsi
	call fb64
	movl $RH_CARD_BOOT_FLAGS, %edi
	xorl %esi, %esi
	call fb64
	movl $RH_HOSTMEM_ADDR, %edi
	movq 8(%rsp), %rsi
	call fb64
	movl $RH_HOSTMEM_SIZE, %edi
	movq 16(%rsp), %rsi
	call fb64
	movl $RH_MAGIC, %edi
	movl $REGION_MAGIC, %esi
	call fb32
	movq %r13, %rax
	addq $40, %rsp
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
90:	leaq s_e_fit(%rip), %rsi
	jmp fail
STR s_e_fit, "the ring layout does not fit the region (--ring-size)"

# format_ring(rdi = offset, esi = size): magic, size, head = tail = 0 in
# the format buffer.
format_ring:
	pushq %rdi
	pushq %rsi
	leaq RG_HEAD(%rdi), %rdi
	xorl %esi, %esi
	call fb32
	movq 8(%rsp), %rdi
	leaq RG_TAIL(%rdi), %rdi
	xorl %esi, %esi
	call fb32
	movq 8(%rsp), %rdi
	leaq RG_SIZE(%rdi), %rdi
	movq (%rsp), %rsi
	call fb32
	movq 8(%rsp), %rdi
	movl $RING_MAGIC, %esi
	call fb32
	popq %rsi
	popq %rdi
	ret

	.globl plan_end
# plan_end() -> rax: the first byte past the default plan's layout.
plan_end:
	movl $REGION_HDR_SIZE + NPLAN * CHANNEL_DESC_SIZE, %eax
	addq $LINE - 1, %rax
	andq $-LINE, %rax
	xorl %ecx, %ecx
1:	cmpl $NPLAN, %ecx
	jae 3f
	leaq default_plan(%rip), %rdx
	movl %ecx, %esi
	shll $4, %esi
	addq %rsi, %rdx
	movl 4(%rdx), %esi
	leaq RING_HDR_SIZE(%rax,%rsi), %rax
	addq $LINE - 1, %rax
	andq $-LINE, %rax
	movl 8(%rdx), %esi
	leaq RING_HDR_SIZE(%rax,%rsi), %rax
	addq $LINE - 1, %rax
	andq $-LINE, %rax
	incl %ecx
	jmp 1b
3:	xorl %ecx, %ecx
4:	cmpl $NPLAN, %ecx
	jae 6f
	leaq default_plan(%rip), %rdx
	movl %ecx, %esi
	shll $4, %esi
	addq %rsi, %rdx
	movl 12(%rdx), %esi
	testl %esi, %esi
	jz 5f
	addq $DATA_ALIGN - 1, %rax
	andq $-DATA_ALIGN, %rax
	addq %rsi, %rax
	addq $DATA_ALIGN - 1, %rax
	andq $-DATA_ALIGN, %rax
5:	incl %ecx
	jmp 4b
6:	ret
