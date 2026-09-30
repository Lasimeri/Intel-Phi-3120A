# card.S: the card as registers and memory (host/crates/phi-hw/src/card.rs
# and phi-vfio's mapping.rs): SBOX reads and writes, the POST code,
# scratchpads, waiting for the bootstrap, the reset, the boot interrupt,
# and the aperture copies. See card.md.
	.include "defs.inc"

	.bss
	.p2align 3
	.globl traffic
# Bytes moved, for `phictl traffic`: DMA to card, DMA from card,
# aperture to card, aperture from card, DMA copies to, DMA copies from.
traffic:	.skip 48
probe:	.skip 16

	.text

	.globl card_open
# card_open(rsi = normalized bdf): the device must be a Phi bound to
# vfio-pci.
card_open:
	pushq %rsi
	call is_phi
	testl %eax, %eax
	jz 8f
	movq (%rsp), %rsi
	call driver_of
	leaq s_vfio_pci(%rip), %rdi
	call eq_cstr
	testl %eax, %eax
	jz 9f
	popq %rsi
	jmp vfio_open
8:	leaq s_e_notphi(%rip), %rsi
	jmp fail
9:	leaq s_e_notbound(%rip), %rsi
	jmp fail
STR s_vfio_pci, "vfio-pci"
STR s_e_notphi, "not a Xeon Phi 3120 series device (vendor 8086 device 225d expected)"
STR s_e_notbound, "the card is not bound to vfio-pci; run: sudo scripts/bind-vfio.sh"

	.globl sbox_read
# sbox_read(edi = SBOX offset) -> eax. One 32-bit uncached access.
sbox_read:
	movq mmio_base(%rip), %rax
	movl SBOX_BASE(%rax,%rdi), %eax
	ret

	.globl sbox_write
# sbox_write(edi = SBOX offset, esi = value).
sbox_write:
	movq mmio_base(%rip), %rax
	movl %esi, SBOX_BASE(%rax,%rdi)
	ret

	.globl postcode
# postcode() -> eax: the POST code register (a DBOX-side BAR4 offset).
postcode:
	movq mmio_base(%rip), %rax
	movl POSTCODE(%rax), %eax
	ret

	.globl spad
# spad(edi = n) -> eax.
spad:
	leal SPAD0(,%rdi,4), %edi
	jmp sbox_read

	.globl set_spad
# set_spad(edi = n, esi = value).
set_spad:
	leal SPAD0(,%rdi,4), %edi
	jmp sbox_write

	.globl download_info
# download_info() -> eax: SPAD2 (bit 0 ready, bits 9:1 APIC id, bits
# 31:12 the download address).
download_info:
	movl $SPAD_DOWNLOAD_INFO, %edi
	jmp spad

	.globl wait_ready
# wait_ready(rdi = timeout ms): poll until POST "12" and SPAD2's ready
# bit, logging every POST change; exits when the timeout passes.
wait_ready:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rdi, %r13
	call now_ms
	addq %rax, %r13
	call postcode
	movl %eax, %ebx
	call l_begin
	leaq s_lg(%rip), %rsi
	call w_cstr
	leaq s_wait1(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call post_text
	call w_bytes
	leaq s_wait2(%rip), %rsi
	call w_cstr
	call l_end
1:	call postcode
	cmpl %ebx, %eax
	je 2f
	movl %eax, %ebx
	movl %eax, %edi
	call log_post
2:	call download_info
	movl %eax, %r12d
	movl %ebx, %edi
	call post_is_ready
	testl %eax, %eax
	jz 3f
	testl $1, %r12d
	jz 3f
	call l_begin
	leaq s_lg(%rip), %rsi
	call w_cstr
	leaq s_ready1(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	andl $0xfffff000, %edi
	call w_hex
	leaq s_ready2(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	shrl $1, %edi
	andl $0x1ff, %edi
	call w_dec
	call l_end
	popq %r13
	popq %r12
	popq %rbx
	ret
3:	call now_ms
	cmpq %r13, %rax
	jae 4f
	movl $50, %edi
	call sleep_ms
	jmp 1b
4:	call l_begin
	leaq s_prefix(%rip), %rsi
	call w_cstr
	leaq s_notready1(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call post_text
	call w_bytes
	leaq s_notready2(%rip), %rsi
	call w_cstr
	movl %r12d, %edi
	movl $8, %esi
	call w_hexw
	leaq s_notready3(%rip), %rsi
	call w_cstr
	call l_end
	movl $1, %edi
	SYS SYS_exit_group
STR s_lg, "[phictl] "
STR s_prefix, "phictl: "
STR s_wait1, "bootstrap at POST \""
STR s_wait2, "\"; waiting for \"12\""
STR s_ready1, "bootstrap ready: download addr "
STR s_ready2, ", BSP APIC id "
STR s_notready1, "bootstrap not ready: POST code \""
STR s_notready2, "\", SPAD2 "
STR s_notready3, " (reset the card first)"

	.globl log_post
# log_post(edi = raw): "[phictl] POST "xx" description".
log_post:
	pushq %rdi
	call l_begin
	leaq s_lg(%rip), %rsi
	call w_cstr
	leaq s_post(%rip), %rsi
	call w_cstr
	movq (%rsp), %rdi
	call post_text
	call w_bytes
	leaq s_quote_sp(%rip), %rsi
	call w_cstr
	popq %rdi
	call post_describe
	call w_cstr
	jmp l_end
STR s_post, "POST \""
STR s_quote_sp, "\" "

	.globl send_icr
# send_icr(edi = n, esi = apic id, edx = vector): the destination first,
# read back to order it, then the low dword with the send bit.
send_icr:
	pushq %rbx
	pushq %r12
	leal APICICR0(,%rdi,8), %ebx
	movl %edx, %r12d
	leal 4(%rbx), %edi
	call sbox_write
	leal 4(%rbx), %edi
	call sbox_read
	movl %ebx, %edi
	movl %r12d, %esi
	andl $0xff, %esi
	orl $ICR_SEND, %esi
	call sbox_write
	movl %ebx, %edi
	call sbox_read
	popq %r12
	popq %rbx
	ret

	.globl send_boot_interrupt
# send_boot_interrupt: vector 229 to the BSP through ICR 7.
send_boot_interrupt:
	call download_info
	movl %eax, %esi
	shrl $1, %esi
	andl $0x1ff, %esi
	movl $7, %edi
	movl $BSP_INTERRUPT_VECTOR, %edx
	jmp send_icr

	.globl aper_write
# aper_write(rdi = card address, rsi = data, rdx = len): a copy into
# the aperture: single bytes up to 8-byte alignment, then 32-byte AVX2
# stores while 32 remain (the source need not be aligned), then 8-byte
# stores, then bytes. Counted in traffic.
aper_write:
	addq %rdx, traffic+16(%rip)
	addq aper_base(%rip), %rdi
1:	testq %rdx, %rdx
	jz 9f
	testq $7, %rdi
	jz 2f
	movb (%rsi), %al
	movb %al, (%rdi)
	incq %rsi
	incq %rdi
	decq %rdx
	jmp 1b
2:	cmpq $32, %rdx
	jb 4f
3:	vmovdqu (%rsi), %ymm0
	vmovdqu %ymm0, (%rdi)
	addq $32, %rsi
	addq $32, %rdi
	subq $32, %rdx
	cmpq $32, %rdx
	jae 3b
	vzeroupper
4:	cmpq $8, %rdx
	jb 6f
5:	movq (%rsi), %rax
	movq %rax, (%rdi)
	addq $8, %rsi
	addq $8, %rdi
	subq $8, %rdx
	cmpq $8, %rdx
	jae 5b
6:	testq %rdx, %rdx
	jz 9f
	movb (%rsi), %al
	movb %al, (%rdi)
	incq %rsi
	incq %rdi
	decq %rdx
	jmp 6b
9:	ret

	.globl aper_read
# aper_read(rdi = card address, rsi = buf, rdx = len): the mirror of
# aper_write. Uncached reads of a BAR are serialised by the fabric
# whatever their width; 8-byte loads keep the transaction count low.
aper_read:
	addq %rdx, traffic+24(%rip)
	addq aper_base(%rip), %rdi
1:	testq %rdx, %rdx
	jz 9f
	testq $7, %rdi
	jz 2f
	movb (%rdi), %al
	movb %al, (%rsi)
	incq %rsi
	incq %rdi
	decq %rdx
	jmp 1b
2:	cmpq $8, %rdx
	jb 4f
3:	movq (%rdi), %rax
	movq %rax, (%rsi)
	addq $8, %rsi
	addq $8, %rdi
	subq $8, %rdx
	cmpq $8, %rdx
	jae 3b
4:	testq %rdx, %rdx
	jz 9f
	movb (%rdi), %al
	movb %al, (%rsi)
	incq %rsi
	incq %rdi
	decq %rdx
	jmp 4b
9:	ret

	.globl aper_read32
# aper_read32(rdi = card address) -> eax: one 32-bit access.
aper_read32:
	movq aper_base(%rip), %rax
	movl (%rax,%rdi), %eax
	ret

	.globl aper_write32
# aper_write32(rdi = card address, esi = value).
aper_write32:
	movq aper_base(%rip), %rax
	movl %esi, (%rax,%rdi)
	ret

	.globl aper_read64
aper_read64:
	movq aper_base(%rip), %rax
	movq (%rax,%rdi), %rax
	ret

	.globl aper_write64
aper_write64:
	movq aper_base(%rip), %rax
	movq %rsi, (%rax,%rdi)
	ret

	.globl aper_check
# aper_check(rdi = card address, rdx = len): exits when the range is
# outside the aperture.
aper_check:
	movq %rdi, %rax
	addq %rdx, %rax
	jc 1f
	cmpq aper_len(%rip), %rax
	ja 1f
	ret
1:	leaq s_e_aper(%rip), %rsi
	jmp fail
STR s_e_aper, "address range exceeds the aperture"

	.globl write_card_memory
# write_card_memory(rdi = card address, rsi = data, rdx = len): the
# paced write path: 4 KiB chunks, the last eight bytes of each read
# back (a non-posted read that bounds the posted writes in flight; an
# unpaced burst reset the host, 2026-09-14) and compared. Exits on a
# range past the aperture or the card's GDDR, or a mismatch.
write_card_memory:
	movl $APERTURE_WRITE_CHUNK, %ecx
	movl $1, %r8d
	pushq %rdi
	pushq %rdx
	movq %rdi, %rax
	addq %rdx, %rax
	movq $GDDR_BYTES, %rdx
	cmpq %rdx, %rax
	ja 8f
	popq %rdx
	popq %rdi
	jmp write_paced
8:	leaq s_e_gddr(%rip), %rsi
	jmp fail
STR s_e_gddr, "write exceeds card GDDR (6 GiB)"

	.globl write_paced
# write_paced(rdi = card address, rsi = data, rdx = len, rcx = chunk,
# r8d = readback flag).
write_paced:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq %rdi, %rbx
	movq %rsi, %r12
	movq %rdx, %r13
	movq %rcx, %r14
	movl %r8d, %r15d
	cmpq $8, %r14
	jae 1f
	movl $8, %r14d
1:	movq %rbx, %rdi
	movq %r13, %rdx
	call aper_check
2:	testq %r13, %r13
	jz 9f
	movq %r14, %rdx
	cmpq %r13, %rdx
	jbe 3f
	movq %r13, %rdx
3:	movq %rbx, %rdi
	movq %r12, %rsi
	pushq %rdx
	call aper_write
	popq %rdx
	testl %r15d, %r15d
	jz 5f
	# read back the last eight bytes of the chunk (fewer for a short one)
	movq %rdx, %rcx
	cmpq $8, %rcx
	jbe 4f
	movl $8, %ecx
4:	leaq (%rbx,%rdx), %rdi
	subq %rcx, %rdi
	pushq %rdx
	pushq %rcx
	leaq probe(%rip), %rsi
	movq %rcx, %rdx
	call aper_read
	popq %rcx
	popq %rdx
	leaq (%r12,%rdx), %rsi
	subq %rcx, %rsi
	leaq probe(%rip), %rdi
	repe cmpsb
	jne 8f
5:	addq %rdx, %rbx
	addq %rdx, %r12
	subq %rdx, %r13
	jmp 2b
8:	leaq s_e_readback(%rip), %rsi
	jmp fail
9:	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_e_readback, "aperture read-back mismatch after a chunk"

	.globl read_card_memory
# read_card_memory(rdi = card address, rsi = buf, rdx = len).
read_card_memory:
	pushq %rdi
	pushq %rsi
	pushq %rdx
	call aper_check
	popq %rdx
	popq %rsi
	popq %rdi
	jmp aper_read

	.globl card_reset
# card_reset(rdi = timeout ms): RGCR bit 0, one second, SPAD2 cleared,
# then wait for the ready bit; every POST change is logged with its
# time. Exits when the timeout passes.
card_reset:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	movq %rdi, %r14
	call now_ms
	movq %rax, %r12			# start
	addq %rax, %r14			# deadline
	call postcode
	movl %eax, %ebx
	movl $RGCR, %edi
	call sbox_read
	movl $RGCR, %edi
	call sbox_read
	movl %eax, %esi
	orl $RGCR_RESET, %esi
	movl $RGCR, %edi
	call sbox_write
	leaq 1000(%r12), %r13		# settle
1:	call sample_post
	call now_ms
	cmpq %r13, %rax
	jae 2f
	movl $10, %edi
	call sleep_ms
	jmp 1b
2:	movl $SPAD_DOWNLOAD_INFO, %edi
	xorl %esi, %esi
	call set_spad
3:	call sample_post
	call download_info
	testl $1, %eax
	jnz 5f
	call now_ms
	cmpq %r14, %rax
	jae 4f
	movl $10, %edi
	call sleep_ms
	jmp 3b
4:	leaq s_e_reset(%rip), %rsi
	jmp fail
5:	call now_ms
	subq %r12, %rax
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
# sample_post: log a POST change with the time since r12 (ebx = last).
sample_post:
	call postcode
	cmpl %ebx, %eax
	je 9f
	movl %eax, %ebx
	pushq %rax
	call l_begin
	leaq s_two(%rip), %rsi
	call w_cstr
	call now_ms
	subq %r12, %rax
	movq %rax, %rdi
	call l_elapsed
	movq (%rsp), %rdi
	movl $8, %esi
	call w_hexw
	leaq s_sp_quote(%rip), %rsi
	call w_cstr
	movq (%rsp), %rdi
	call post_text
	call w_bytes
	leaq s_quote_sp(%rip), %rsi
	call w_cstr
	popq %rdi
	call post_describe
	call w_cstr
	call l_end
9:	ret
STR s_e_reset, "bootstrap not ready after the reset (timeout)"
STR s_two, "  "
STR s_sp_quote, " \""
