# disk.S: the block services (host/crates/phictl/src/disk.rs): `--disk
# IMG` as the card's /dev/phiblk0 on ring channel kind 4 and `--host-mem`
# as /dev/phiblk1 on kind 5. The card posts 32-byte request records in
# the card-to-host ring; the host moves the bytes between the backing
# store and card memory (the DMA engine, or the aperture when the engine
# is unusable) and answers each record with an 8-byte completion. Host
# memory on the DMA path is pipelined: records are submitted as they
# arrive and answered as their copies complete, up to 63 in flight;
# everything else is served one at a time in order. See disk.md.
	.include "defs.inc"

	.set REQ_SIZE, 32
	.set CPL_SIZE, 8
	.set OP_READ, 0
	.set OP_WRITE, 1
	.set OP_FLUSH, 2
	.set OP_IDENT, 3
	.set IDENT_TAG, 0xffffffff
	.set IDENT_TAG_DIRECT, 0xfffffffe
	.set SECTOR, 512
	.set MAX_LEN, 524288
	.set DMA_RING_IOVA, 0x10000000
	.set DMA_DATA_IOVA, 0x10100000
	.set SELFTEST_OFFSET, 0xe00000
	.set STATUS_OFFSET, 0xfff000
	.set MAX_IN_FLIGHT, 63
	.set RECORDS_PER_STEP, 64

# A service: 0 kind, 8 channel number, 16 backend (1 file, 2 ram), 24
# fd, 32 ram pointer, 40 ram length, 48 ram card address, 56 capacity
# (sectors), 64 c2h ring offset, 72 c2h size, 80 h2c ring offset, 88
# h2c size, 96 ready, 104 path (1 dma, 2 aperture), 112 staging host
# pointer, 120 staging card address, 128 in-flight head, 136 in-flight
# tail, 144 anomalies, 152 trace left, 160 what (name), 168 the DMA
# channel (DC_SIZE), 256 reads, 264 writes, 272 flushes, 280 bytes read,
# 288 bytes written, 296 errors; 320 bytes.
	.set SV_KIND, 0
	.set SV_CHAN, 8
	.set SV_BACKEND, 16
	.set SV_FD, 24
	.set SV_RAM, 32
	.set SV_RAM_LEN, 40
	.set SV_RAM_CARD, 48
	.set SV_CAPACITY, 56
	.set SV_C2H, 64
	.set SV_C2H_SIZE, 72
	.set SV_H2C, 80
	.set SV_H2C_SIZE, 88
	.set SV_READY, 96
	.set SV_PATH, 104
	.set SV_STAGING, 112
	.set SV_STAGING_CARD, 120
	.set SV_IF_HEAD, 128
	.set SV_IF_TAIL, 136
	.set SV_ANOMALIES, 144
	.set SV_TRACE, 152
	.set SV_WHAT, 160
	.set SV_DMA, 168
	.set SV_READS, 256
	.set SV_WRITES, 264
	.set SV_FLUSHES, 272
	.set SV_BYTES_READ, 280
	.set SV_BYTES_WRITTEN, 288
	.set SV_ERRORS, 296
	.set SV_SIZE, 320
	.set B_FILE, 1
	.set B_RAM, 2
	.set P_DMA, 1
	.set P_APER, 2

	.bss
	.p2align 6
disk_sv:	.skip SV_SIZE
hostmem_sv:	.skip SV_SIZE
# In flight per service: 16-byte entries (tag, seq), a ring of 64.
disk_if:	.skip 64 * 16
hostmem_if:	.skip 64 * 16
	.p2align 6
aper_buf:	.skip MAX_LEN
rec:	.skip 32
pattern:	.skip 65536
back:	.skip 65536
	.globl disk_bounce
disk_bounce:	.skip 8

	.text

	.globl disk_step
disk_step:
	leaq disk_sv(%rip), %rdi
	cmpq $0, SV_READY(%rdi)
	jne blk_step
	movq $KIND_BLOCK, SV_KIND(%rdi)
	movq $0, SV_CHAN(%rdi)
	leaq s_what_disk(%rip), %rax
	movq %rax, SV_WHAT(%rdi)
	jmp blk_open

	.globl hostmem_step
hostmem_step:
	leaq hostmem_sv(%rip), %rdi
	cmpq $0, SV_READY(%rdi)
	jne blk_step
	movq $KIND_HOSTMEM, SV_KIND(%rdi)
	movq $1, SV_CHAN(%rdi)
	leaq s_what_hostmem(%rip), %rax
	movq %rax, SV_WHAT(%rdi)
	jmp blk_open
STR s_what_disk, "the disk image"
STR s_what_hostmem, "host memory"

# if_buf(rbx = service) -> rax = its in-flight ring.
if_buf:
	leaq disk_if(%rip), %rax
	leaq disk_sv(%rip), %rcx
	cmpq %rcx, %rbx
	je 1f
	leaq hostmem_if(%rip), %rax
1:	ret

# blk_open(rdi = service) -> eax 1 when it just came up (0 while the
# card is not ready): the backend, the rings, the data path.
blk_open:
	pushq %rbx
	pushq %r12
	movq %rdi, %rbx
	call wait_init_seen
	testl %eax, %eax
	jz 90f
	cmpq $0, region_ok(%rip)
	je 90f
	movl SV_KIND(%rbx), %edi
	call endpoints_of
	testl %eax, %eax
	jz 90f
	movq %r8, SV_H2C(%rbx)
	movq %r9, SV_H2C_SIZE(%rbx)
	movq %r10, SV_C2H(%rbx)
	movq %r11, SV_C2H_SIZE(%rbx)
	# the backend
	cmpq $KIND_BLOCK, SV_KIND(%rbx)
	jne 2f
	movq $B_FILE, SV_BACKEND(%rbx)
	movq o_disk(%rip), %rdi
	movl $O_RDWR | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	js 91f
	movq %rax, SV_FD(%rbx)
	subq $152, %rsp
	movl %eax, %edi
	movq %rsp, %rsi
	SYS SYS_fstat
	movq 48(%rsp), %rax
	addq $152, %rsp
	shrq $9, %rax
	movq %rax, SV_CAPACITY(%rbx)
	jmp 3f
2:	movq $B_RAM, SV_BACKEND(%rbx)
	movq hostmem_ptr(%rip), %rax
	movq %rax, SV_RAM(%rbx)
	movq hostmem_len(%rip), %rax
	movq %rax, SV_RAM_LEN(%rbx)
	shrq $9, %rax
	movq %rax, SV_CAPACITY(%rbx)
	movq hostmem_card(%rip), %rax
	movq %rax, SV_RAM_CARD(%rbx)
3:	call l_begin
	call l_tag
	leaq s_serving1(%rip), %rsi
	call w_cstr
	movq SV_WHAT(%rbx), %rsi
	call w_cstr
	leaq s_serving2(%rip), %rsi
	call w_cstr
	movq SV_CAPACITY(%rbx), %rdi
	shrq $11, %rdi
	call w_dec
	leaq s_serving3(%rip), %rsi
	call w_cstr
	movq SV_KIND(%rbx), %rdi
	call w_dec
	call l_end
	movq $P_APER, SV_PATH(%rbx)
	cmpq $0, o_no_dma(%rip)
	jne 4f
	movq %rbx, %rdi
	call open_dma_path
4:	movq $0, SV_IF_HEAD(%rbx)
	movq $0, SV_IF_TAIL(%rbx)
	movq $0, SV_ANOMALIES(%rbx)
	leaq s_env_trace(%rip), %rdi
	call getenv
	xorl %ecx, %ecx
	testq %rax, %rax
	jz 5f
	movl $40, %ecx
5:	movq %rcx, SV_TRACE(%rbx)
	leaq s_env_bounce(%rip), %rdi
	call getenv
	testq %rax, %rax
	setnz %al
	movzbl %al, %eax
	movq %rax, disk_bounce(%rip)
	movq $1, SV_READY(%rbx)
	movl $1, %eax
	jmp 99f
90:	xorl %eax, %eax
	jmp 99f
91:	movq o_disk(%rip), %rsi
	jmp fail_neg
99:	popq %r12
	popq %rbx
	ret
STR s_serving1, "disk: serving "
STR s_serving2, " ("
STR s_serving3, " MiB) on channel kind "
STR s_env_trace, "PHICTL_DISK_TRACE"
STR s_env_bounce, "PHICTL_DISK_BOUNCE"

# open_dma_path(rdi = service): the engine brought up on the service's
# channel and proved with a round trip through card memory the card does
# not use (the last 2 MiB of the region): a pattern into the card by DMA
# read back through the aperture, the inverse pattern in through the
# aperture read back by DMA, then 16 round trips of MAX_LEN for the rate.
# Anything failing leaves the aperture path.
open_dma_path:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	movq %rdi, %rbx
	leaq SV_DMA(%rbx), %rdi
	movl SV_CHAN(%rbx), %esi
	movq %rsi, %rdx
	shlq $13, %rdx
	addq $DMA_RING_IOVA, %rdx
	movq SV_CHAN(%rbx), %rcx
	shlq $6, %rcx
	addq region_base(%rip), %rcx
	addq $STATUS_OFFSET, %rcx
	call dma_open
	testl %eax, %eax
	jz 99f
	movq SV_CHAN(%rbx), %rdi
	shlq $20, %rdi
	addq $DMA_DATA_IOVA, %rdi
	movq %rdi, %r14
	movl $MAX_LEN, %esi
	call dma_buffer
	testq %rax, %rax
	js 90f
	movq %rax, SV_STAGING(%rbx)
	movq %r14, %rdi
	call card_addr_of
	movq %rax, SV_STAGING_CARD(%rbx)
	# scratch in card memory
	movq SV_CHAN(%rbx), %r12
	shlq $20, %r12
	addq region_base(%rip), %r12
	addq $SELFTEST_OFFSET, %r12
	# the pattern: byte i = (i * 2654435761) ^ (i >> 8)
	xorl %ecx, %ecx
1:	movl %ecx, %eax
	imull $2654435761, %eax, %eax
	movl %ecx, %edx
	shrl $8, %edx
	xorl %edx, %eax
	leaq pattern(%rip), %rdi
	movb %al, (%rdi,%rcx)
	incl %ecx
	cmpl $65536, %ecx
	jb 1b
	movq SV_STAGING(%rbx), %rdi
	leaq pattern(%rip), %rsi
	movl $65536, %ecx
	rep movsb
	leaq SV_DMA(%rbx), %rdi
	movq SV_STAGING_CARD(%rbx), %rsi
	movq %r12, %rdx
	movl $65536, %ecx
	call dma_copy
	testl %eax, %eax
	jz 91f
	movq %r12, %rdi
	leaq back(%rip), %rsi
	movl $65536, %edx
	call read_card_memory
	leaq pattern(%rip), %rsi
	leaq back(%rip), %rdi
	movl $65536, %ecx
	repe cmpsb
	jne 92f
	# the inverse in through the aperture, out by DMA
	xorl %ecx, %ecx
2:	leaq pattern(%rip), %rdi
	movb (%rdi,%rcx), %al
	notb %al
	leaq back(%rip), %rdi
	movb %al, (%rdi,%rcx)
	incl %ecx
	cmpl $65536, %ecx
	jb 2b
	leaq 65536(%r12), %rdi
	leaq back(%rip), %rsi
	movl $65536, %edx
	call write_card_memory
	movq SV_STAGING(%rbx), %rdi
	xorl %eax, %eax
	movl $65536, %ecx
	rep stosb
	leaq SV_DMA(%rbx), %rdi
	leaq 65536(%r12), %rsi
	movq SV_STAGING_CARD(%rbx), %rdx
	movl $65536, %ecx
	call dma_copy
	testl %eax, %eax
	jz 93f
	movq SV_STAGING(%rbx), %rsi
	leaq back(%rip), %rdi
	movl $65536, %ecx
	repe cmpsb
	jne 94f
	# the rate: 16 round trips of MAX_LEN
	call now_us
	movq %rax, %r13
	movl $16, %r14d
3:	leaq SV_DMA(%rbx), %rdi
	movq SV_STAGING_CARD(%rbx), %rsi
	movq %r12, %rdx
	movl $MAX_LEN, %ecx
	call dma_copy
	testl %eax, %eax
	jz 95f
	leaq SV_DMA(%rbx), %rdi
	movq %r12, %rsi
	movq SV_STAGING_CARD(%rbx), %rdx
	movl $MAX_LEN, %ecx
	call dma_copy
	testl %eax, %eax
	jz 95f
	decl %r14d
	jnz 3b
	call now_us
	subq %r13, %rax
	movq %rax, %rcx		# microseconds
	movq $32 * MAX_LEN, %rax	# bytes; MB/s = bytes / us
	xorl %edx, %edx
	testq %rcx, %rcx
	jz 4f
	divq %rcx
4:	pushq %rax
	movq $P_DMA, SV_PATH(%rbx)
	call l_begin
	call l_tag
	leaq s_verified1(%rip), %rsi
	call w_cstr
	movq SV_CHAN(%rbx), %rdi
	call w_dec
	leaq s_verified2(%rip), %rsi
	call w_cstr
	popq %rdi
	call w_dec
	leaq s_verified3(%rip), %rsi
	call w_cstr
	call l_end
	jmp 99f
90:	leaq s_st_buf(%rip), %rsi
	jmp 98f
91:	leaq s_st_h2c(%rip), %rsi
	jmp 98f
92:	leaq s_st_h2c_data(%rip), %rsi
	jmp 98f
93:	leaq s_st_c2h(%rip), %rsi
	jmp 98f
94:	leaq s_st_c2h_data(%rip), %rsi
	jmp 98f
95:	leaq s_st_rate(%rip), %rsi
98:	pushq %rsi
	call l_begin
	call l_tag
	leaq s_st_failed(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	leaq s_st_aper(%rip), %rsi
	call w_cstr
	call l_end
	leaq SV_DMA(%rbx), %rdi
	call dma_close
	movq $P_APER, SV_PATH(%rbx)
99:	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_verified1, "disk: DMA engine channel "
STR s_verified2, " verified, "
STR s_verified3, " MB/s in the self-test"
STR s_st_failed, "disk: DMA self-test failed ("
STR s_st_aper, "); copying through the aperture"
STR s_st_buf, "the staging buffer could not be pinned"
STR s_st_h2c, "the host to card copy did not complete"
STR s_st_h2c_data, "host to card DMA delivered wrong data"
STR s_st_c2h, "the card to host copy did not complete"
STR s_st_c2h_data, "card to host DMA delivered wrong data"
STR s_st_rate, "a copy of the rate test did not complete"

# ------------------------------------------------------------ the pass

# blk_step(rdi = service) -> eax 1 when a record was served, a copy
# completed or copies are in flight.
blk_step:
	pushq %rbx
	pushq %r12
	pushq %r14
	pushq %r15
	subq $8, %rsp
	movq %rdi, %rbx
	xorl %r15d, %r15d
	movl $RECORDS_PER_STEP, %r14d
1:	# copies the engine finished since the last look
	movq SV_IF_TAIL(%rbx), %rax
	cmpq SV_IF_HEAD(%rbx), %rax
	je 2f
	movl $1, %r15d
	cmpq $P_DMA, SV_PATH(%rbx)
	jne 2f
	leaq SV_DMA(%rbx), %rdi
	call dma_poll
	testq %rax, %rax
	js 11f
	movq %rax, %rsi
	movq %rbx, %rdi
	call complete_done
	jmp 2f
11:	movq %rbx, %rdi
	call fail_all
2:	# the indices, raw: an impossible count is logged and re-read, never
	# resynchronised (a dropped block request hangs the card)
	movq SV_C2H(%rbx), %rdi
	addq $RG_HEAD, %rdi
	call rm_read32
	movl %eax, %r12d
	movq SV_C2H(%rbx), %rdi
	addq $RG_TAIL, %rdi
	call rm_read32
	movl %r12d, %ecx
	subl %eax, %ecx		# avail
	cmpq SV_C2H_SIZE(%rbx), %rcx
	jbe 3f
	incq SV_ANOMALIES(%rbx)
	movq SV_ANOMALIES(%rbx), %rax
	cmpq $5, %rax
	jbe 21f
	leaq -1(%rax), %rcx
	testq %rcx, %rax
	jnz 22f
21:	pushq %r12
	pushq %rax
	call l_begin
	call l_tag
	leaq s_impossible1(%rip), %rsi
	call w_cstr
	movq SV_WHAT(%rbx), %rsi
	call w_cstr
	leaq s_impossible2(%rip), %rsi
	call w_cstr
	movq 8(%rsp), %rdi
	call w_hex
	leaq s_impossible3(%rip), %rsi
	call w_cstr
	movq SV_ANOMALIES(%rbx), %rdi
	call w_dec
	movb $')', %dil
	call w_byte
	call l_end
	addq $16, %rsp
22:	movl $200, %edi
	call sleep_us
	jmp 9f
3:	cmpl $REQ_SIZE, %ecx
	jb 9f
	movq SV_C2H(%rbx), %rdi
	movl SV_C2H_SIZE(%rbx), %esi
	leaq rec(%rip), %rdx
	movl $REQ_SIZE, %ecx
	call ring_pop
	cmpq $REQ_SIZE, %rax
	je 4f
	incq SV_ANOMALIES(%rbx)
	call l_begin
	call l_tag
	leaq s_short_rec(%rip), %rsi
	call w_cstr
	call l_end
	jmp 9f
4:	movl $1, %r15d
	cmpq $0, SV_TRACE(%rbx)
	je 5f
	decq SV_TRACE(%rbx)
	call trace_record
5:	# the pipelined path: host memory over the engine, read or write
	cmpq $B_RAM, SV_BACKEND(%rbx)
	jne 6f
	cmpq $P_DMA, SV_PATH(%rbx)
	jne 6f
	movl rec+4(%rip), %eax
	cmpl $OP_WRITE, %eax
	ja 6f
	movq %rbx, %rdi
	call submit_ram
	jmp 8f
6:	# everything else keeps its place: the pipeline drains first
	movq SV_IF_TAIL(%rbx), %rax
	cmpq SV_IF_HEAD(%rbx), %rax
	je 7f
	movq %rbx, %rdi
	call if_buf
	movq SV_IF_TAIL(%rbx), %rcx
	decq %rcx
	andq $63, %rcx
	shlq $4, %rcx
	movq 8(%rax,%rcx), %rsi	# the last seq
	leaq SV_DMA(%rbx), %rdi
	pushq %rsi
	call dma_wait
	popq %rsi
	testl %eax, %eax
	jz 61f
	movq %rbx, %rdi
	call complete_done
	jmp 7f
61:	movq %rbx, %rdi
	call fail_all
7:	movq %rbx, %rdi
	call serve_one
	movl %eax, %edx		# status
	movl rec(%rip), %esi	# tag
	cmpl $OP_IDENT, rec+4(%rip)
	jne 71f
	movl $IDENT_TAG_DIRECT, %esi
	cmpq $0, disk_bounce(%rip)
	je 71f
	movl $IDENT_TAG, %esi
71:	movq %rbx, %rdi
	call complete
8:	decl %r14d
	jnz 1b
9:	movl %r15d, %eax
	addq $8, %rsp
	popq %r15
	popq %r14
	popq %r12
	popq %rbx
	ret
STR s_impossible1, "disk: "
STR s_impossible2, ": impossible request count (head "
STR s_impossible3, "); re-reading (anomaly "
STR s_short_rec, "disk: popped fewer bytes than a record; anomaly"

# trace_record: "[phictl] disk: rec 32 bytes tag 0x.. op N len N sector N phys 0x..".
trace_record:
	call l_begin
	call l_tag
	leaq s_tr1(%rip), %rsi
	call w_cstr
	movl rec(%rip), %edi
	call w_hex
	leaq s_tr2(%rip), %rsi
	call w_cstr
	movl rec+4(%rip), %edi
	call w_dec
	leaq s_tr3(%rip), %rsi
	call w_cstr
	movl rec+8(%rip), %edi
	call w_dec
	leaq s_tr4(%rip), %rsi
	call w_cstr
	movq rec+16(%rip), %rdi
	call w_dec
	leaq s_tr5(%rip), %rsi
	call w_cstr
	movq rec+24(%rip), %rdi
	call w_hex
	jmp l_end
STR s_tr1, "disk: rec 32 bytes tag "
STR s_tr2, " op "
STR s_tr3, " len "
STR s_tr4, " sector "
STR s_tr5, " phys "

# check_record(rbx = service) -> rax = the data length, or -1 with the
# status EINVAL: length non-zero, at most MAX_LEN, sector-granular, and
# inside the capacity.
check_record:
	movl rec+8(%rip), %eax
	testl %eax, %eax
	jz 8f
	cmpl $MAX_LEN, %eax
	ja 8f
	testl $SECTOR - 1, %eax
	jnz 8f
	movq %rax, %rcx
	shrq $9, %rcx
	addq rec+16(%rip), %rcx
	jc 8f
	cmpq SV_CAPACITY(%rbx), %rcx
	ja 8f
	ret
8:	movq $-1, %rax
	ret

# submit_ram(rdi = service): a read or write of host memory submitted to
# the engine; the record is answered when its copy completes.
submit_ram:
	pushq %rbx
	pushq %r12
	movq %rdi, %rbx
	call check_record
	testq %rax, %rax
	js 8f
	movq %rax, %r12
	movq rec+16(%rip), %rax
	shlq $9, %rax
	addq SV_RAM_CARD(%rbx), %rax	# the window's card address + offset
	cmpl $OP_READ, rec+4(%rip)
	jne 1f
	incq SV_READS(%rbx)
	addq %r12, SV_BYTES_READ(%rbx)
	movq %rax, %rsi
	movq rec+24(%rip), %rdx
	jmp 2f
1:	incq SV_WRITES(%rbx)
	addq %r12, SV_BYTES_WRITTEN(%rbx)
	movq rec+24(%rip), %rsi
	movq %rax, %rdx
2:	leaq SV_DMA(%rbx), %rdi
	movq %r12, %rcx
	call dma_submit
	testq %rax, %rax
	jz 9f
	movq %rax, %r12			# seq
	movq %rbx, %rdi
	call if_buf
	movq SV_IF_HEAD(%rbx), %rcx
	andq $63, %rcx
	shlq $4, %rcx
	movl rec(%rip), %edx
	movq %rdx, (%rax,%rcx)
	movq %r12, 8(%rax,%rcx)
	incq SV_IF_HEAD(%rbx)
	popq %r12
	popq %rbx
	ret
8:	incq SV_ERRORS(%rbx)
	movq %rbx, %rdi
	movl rec(%rip), %esi
	movl $EINVAL, %edx
	call complete
	popq %r12
	popq %rbx
	ret
9:	incq SV_ERRORS(%rbx)
	movq %rbx, %rdi
	movl rec(%rip), %esi
	movl $EIO, %edx
	call complete
	popq %r12
	popq %rbx
	ret

# complete(rdi = service, esi = tag, edx = status): the 8-byte completion
# into the host-to-card ring, waiting for room (logged at 100 ms and
# every 10 s of waiting).
complete:
	pushq %rbx
	pushq %r12
	subq $16, %rsp
	movq %rdi, %rbx
	movl %esi, (%rsp)
	movl %edx, 4(%rsp)
	xorl %r12d, %r12d		# waited ms
1:	movq SV_H2C(%rbx), %rdi
	movl SV_H2C_SIZE(%rbx), %esi
	call ring_free
	cmpl $CPL_SIZE, %eax
	jae 3f
	movl $1, %edi
	call sleep_ms
	incl %r12d
	cmpl $100, %r12d
	je 2f
	movl %r12d, %eax
	xorl %edx, %edx
	movl $10000, %ecx
	divl %ecx
	testl %edx, %edx
	jnz 1b
2:	incq SV_ANOMALIES(%rbx)
	call l_begin
	call l_tag
	leaq s_full1(%rip), %rsi
	call w_cstr
	movq SV_WHAT(%rbx), %rsi
	call w_cstr
	leaq s_full2(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	call w_dec
	leaq s_full3(%rip), %rsi
	call w_cstr
	movq SV_ANOMALIES(%rbx), %rdi
	call w_dec
	call l_end
	jmp 1b
3:	movq SV_H2C(%rbx), %rdi
	movl SV_H2C_SIZE(%rbx), %esi
	movq %rsp, %rdx
	movl $CPL_SIZE, %ecx
	call ring_push
	addq $16, %rsp
	popq %r12
	popq %rbx
	ret
STR s_full1, "disk: "
STR s_full2, ": completion ring full for "
STR s_full3, " ms; anomaly "

# complete_done(rdi = service, rsi = last completed seq): every record
# whose copy is done, oldest first, answered with 0.
complete_done:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rdi, %rbx
	movq %rsi, %r12
	movq %rbx, %rdi
	call if_buf
	movq %rax, %r13
1:	movq SV_IF_TAIL(%rbx), %rcx
	cmpq SV_IF_HEAD(%rbx), %rcx
	jae 9f
	andq $63, %rcx
	shlq $4, %rcx
	cmpq %r12, 8(%r13,%rcx)
	ja 9f
	movl (%r13,%rcx), %esi
	incq SV_IF_TAIL(%rbx)
	movq %rbx, %rdi
	xorl %edx, %edx
	call complete
	jmp 1b
9:	popq %r13
	popq %r12
	popq %rbx
	ret

# fail_all(rdi = service): the engine failed or stalled: every record in
# flight answered with EIO, once said.
fail_all:
	pushq %rbx
	pushq %r13
	movq %rdi, %rbx
	call l_begin
	call l_tag
	leaq s_failing1(%rip), %rsi
	call w_cstr
	movq SV_WHAT(%rbx), %rsi
	call w_cstr
	leaq s_failing2(%rip), %rsi
	call w_cstr
	movq SV_IF_HEAD(%rbx), %rdi
	subq SV_IF_TAIL(%rbx), %rdi
	call w_dec
	leaq s_failing3(%rip), %rsi
	call w_cstr
	call l_end
	movq %rbx, %rdi
	call if_buf
	movq %rax, %r13
1:	movq SV_IF_TAIL(%rbx), %rcx
	cmpq SV_IF_HEAD(%rbx), %rcx
	jae 9f
	andq $63, %rcx
	shlq $4, %rcx
	movl (%r13,%rcx), %esi
	incq SV_IF_TAIL(%rbx)
	incq SV_ERRORS(%rbx)
	movq %rbx, %rdi
	movl $EIO, %edx
	call complete
	jmp 1b
9:	popq %r13
	popq %rbx
	ret
STR s_failing1, "disk: "
STR s_failing2, ": DMA engine failed; failing "
STR s_failing3, " record(s) in flight"

# serve_one(rdi = service) -> eax = the completion status for the record
# in rec: the capacity for identify (clipped to u32), fdatasync for
# flush, the bytes moved for a read or write, EOPNOTSUPP otherwise.
serve_one:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	movq %rdi, %rbx
	movl rec+4(%rip), %eax
	cmpl $OP_IDENT, %eax
	jne 1f
	movq SV_CAPACITY(%rbx), %rax
	movl $0xffffffff, %ecx
	cmpq %rcx, %rax
	jbe 99f
	movl %ecx, %eax
	jmp 99f
1:	cmpl $OP_FLUSH, %eax
	jne 2f
	incq SV_FLUSHES(%rbx)
	xorl %eax, %eax
	cmpq $B_FILE, SV_BACKEND(%rbx)
	jne 99f
	movl SV_FD(%rbx), %edi
	SYS SYS_fdatasync
	testq %rax, %rax
	jns 11f
	movl $EIO, %eax
	jmp 99f
11:	xorl %eax, %eax
	jmp 99f
2:	cmpl $OP_WRITE, %eax
	ja 98f
	call check_record
	testq %rax, %rax
	js 97f
	movq %rax, %r12			# len
	movq rec+16(%rip), %r13
	shlq $9, %r13			# offset
	cmpl $OP_READ, rec+4(%rip)
	jne 3f
	incq SV_READS(%rbx)
	addq %r12, SV_BYTES_READ(%rbx)
	movq %rbx, %rdi
	call do_read
	jmp 4f
3:	incq SV_WRITES(%rbx)
	addq %r12, SV_BYTES_WRITTEN(%rbx)
	movq %rbx, %rdi
	call do_write
4:	testl %eax, %eax
	jnz 5f
	xorl %eax, %eax
	jmp 99f
5:	incq SV_ERRORS(%rbx)
	call l_begin
	call l_tag
	leaq s_io1(%rip), %rsi
	call w_cstr
	leaq s_io_read(%rip), %rsi
	cmpl $OP_READ, rec+4(%rip)
	je 51f
	leaq s_io_write(%rip), %rsi
51:	call w_cstr
	leaq s_io2(%rip), %rsi
	call w_cstr
	movq rec+16(%rip), %rdi
	call w_dec
	leaq s_io3(%rip), %rsi
	call w_cstr
	movq %r12, %rdi
	call w_dec
	leaq s_io4(%rip), %rsi
	call w_cstr
	call l_end
	movl $EIO, %eax
	jmp 99f
97:	incq SV_ERRORS(%rbx)
	movl $EINVAL, %eax
	jmp 99f
98:	movl $EOPNOTSUPP, %eax
99:	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_io1, "disk: "
STR s_io_read, "read"
STR s_io_write, "write"
STR s_io2, " at sector "
STR s_io3, " ("
STR s_io4, " bytes) failed"

# do_read(rbx = service, r12 = len, r13 = byte offset) -> eax 0 when the
# bytes reached the card at rec.phys: the backing store into the staging
# buffer (or straight from the window) then the engine, or into a host
# buffer then the aperture.
do_read:
	cmpq $B_RAM, SV_BACKEND(%rbx)
	je 3f
	cmpq $P_DMA, SV_PATH(%rbx)
	jne 2f
	movl SV_FD(%rbx), %edi
	movq SV_STAGING(%rbx), %rsi
	movq %r12, %rdx
	movq %r13, %r10
	call pread_all
	testl %eax, %eax
	jnz 9f
	leaq SV_DMA(%rbx), %rdi
	movq SV_STAGING_CARD(%rbx), %rsi
	movq rec+24(%rip), %rdx
	movq %r12, %rcx
	call dma_copy
	testl %eax, %eax
	jz 9f
	xorl %eax, %eax
	ret
2:	movl SV_FD(%rbx), %edi
	leaq aper_buf(%rip), %rsi
	movq %r12, %rdx
	movq %r13, %r10
	call pread_all
	testl %eax, %eax
	jnz 9f
	movq rec+24(%rip), %rdi
	leaq aper_buf(%rip), %rsi
	movq %r12, %rdx
	call write_card_memory
	xorl %eax, %eax
	ret
3:	# host memory on the aperture path (the engine path is pipelined)
	movq rec+24(%rip), %rdi
	movq SV_RAM(%rbx), %rsi
	addq %r13, %rsi
	movq %r12, %rdx
	call write_card_memory
	xorl %eax, %eax
	ret
9:	movl $1, %eax
	ret

# do_write(rbx, r12, r13) -> eax 0 when the card's bytes reached the store.
do_write:
	cmpq $B_RAM, SV_BACKEND(%rbx)
	je 3f
	cmpq $P_DMA, SV_PATH(%rbx)
	jne 2f
	leaq SV_DMA(%rbx), %rdi
	movq rec+24(%rip), %rsi
	movq SV_STAGING_CARD(%rbx), %rdx
	movq %r12, %rcx
	call dma_copy
	testl %eax, %eax
	jz 9f
	movl SV_FD(%rbx), %edi
	movq SV_STAGING(%rbx), %rsi
	movq %r12, %rdx
	movq %r13, %r10
	call pwrite_all
	ret
2:	movq rec+24(%rip), %rdi
	leaq aper_buf(%rip), %rsi
	movq %r12, %rdx
	call read_card_memory
	movl SV_FD(%rbx), %edi
	leaq aper_buf(%rip), %rsi
	movq %r12, %rdx
	movq %r13, %r10
	call pwrite_all
	ret
3:	movq rec+24(%rip), %rdi
	movq SV_RAM(%rbx), %rsi
	addq %r13, %rsi
	movq %r12, %rdx
	call read_card_memory
	xorl %eax, %eax
	ret
9:	movl $1, %eax
	ret

# pread_all(edi = fd, rsi = buf, rdx = len, r10 = offset) -> eax 0 when
# every byte was read (a short file is an error, as read_exact_at).
pread_all:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	movl %edi, %ebx
	movq %rsi, %r12
	movq %rdx, %r13
	movq %r10, %r14
1:	testq %r13, %r13
	jz 3f
	movl %ebx, %edi
	movq %r12, %rsi
	movq %r13, %rdx
	movq %r14, %r10
	SYS SYS_pread64
	testq %rax, %rax
	jz 4f
	js 2f
	addq %rax, %r12
	subq %rax, %r13
	addq %rax, %r14
	jmp 1b
2:	cmpq $-EINTR, %rax
	je 1b
4:	movl $1, %eax
	jmp 5f
3:	xorl %eax, %eax
5:	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# pwrite_all(edi = fd, rsi = buf, rdx = len, r10 = offset) -> eax 0.
pwrite_all:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	movl %edi, %ebx
	movq %rsi, %r12
	movq %rdx, %r13
	movq %r10, %r14
1:	testq %r13, %r13
	jz 3f
	movl %ebx, %edi
	movq %r12, %rsi
	movq %r13, %rdx
	movq %r14, %r10
	SYS SYS_pwrite64
	testq %rax, %rax
	jz 4f
	js 2f
	addq %rax, %r12
	subq %rax, %r13
	addq %rax, %r14
	jmp 1b
2:	cmpq $-EINTR, %rax
	je 1b
4:	movl $1, %eax
	jmp 5f
3:	xorl %eax, %eax
5:	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
