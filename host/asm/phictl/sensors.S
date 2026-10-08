# sensors.S: the SensorsReply text (the sensors_text of phictl's
# serve.rs, through phi-regs sbox::sensors): the die and board
# temperatures, the core voltage and the core clock, read from the
# SBOX for a client's Sensors request, which the relay (serve.S)
# answers without the card. See sensors.md.
	.include "defs.inc"

	.text

	.globl sensors_frame
# sensors_frame: SensorsReply in tx_buf: the text the Rust daemon built
# (serve.rs sensors_text), from the SBOX through the decoders of
# phi-regs sbox::sensors.
sensors_frame:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	subq $48, %rsp
	movl $TAG_SENSORS_REPLY, %edi
	call f_begin
	call w_str_begin
	# die temperatures: three registers, three 10-bit fields each
	leaq s_die1(%rip), %rsi
	call w_cstr
	xorl %ebx, %ebx
1:	cmpl $9, %ebx
	jae 3f
	movl %ebx, %eax
	xorl %edx, %edx
	movl $3, %ecx
	divl %ecx
	leal CURRENT_DIE_TEMP0(,%rax,4), %edi
	movl %edx, %r12d
	call sbox_read
	leal (%r12,%r12,4), %ecx
	addl %ecx, %ecx		# 10 * (i % 3)
	shrl %cl, %eax
	andl $0x3ff, %eax
	testl %ebx, %ebx
	jz 2f
	pushq %rax
	movb $' ', %dil
	call w_byte
	popq %rax
2:	testl %eax, %eax
	jnz 21f
	leaq s_na(%rip), %rsi
	call w_cstr
	jmp 22f
21:	movl %eax, %edi
	call w_dec
22:	incl %ebx
	jmp 1b
3:	leaq s_die2(%rip), %rsi
	call w_cstr
	# the highest of the nine maxima
	xorl %ebx, %ebx
	xorl %r13d, %r13d
4:	cmpl $9, %ebx
	jae 5f
	movl %ebx, %eax
	xorl %edx, %edx
	movl $3, %ecx
	divl %ecx
	leal MAX_DIE_TEMP0(,%rax,4), %edi
	movl %edx, %r12d
	call sbox_read
	leal (%r12,%r12,4), %ecx
	addl %ecx, %ecx
	shrl %cl, %eax
	andl $0x3ff, %eax
	cmpl %r13d, %eax
	jbe 41f
	movl %eax, %r13d
41:	incl %ebx
	jmp 4b
5:	movl %r13d, %edi
	call w_dec
	leaq s_die3(%rip), %rsi
	call w_cstr
	# board: inlet, vccp regulator (BOARD_TEMP1), gddr, gddr regulator
	# (BOARD_TEMP2), vddg (FAN2 bits 19:12), tmu (THERMAL_STATUS)
	leaq s_board1(%rip), %rsi
	call w_cstr
	movl $BOARD_TEMP1, %edi
	call sbox_read
	movl %eax, %ebx
	movl %eax, %edi
	movl $15, %esi
	call w_opt_temp
	leaq s_board2(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	shrl $16, %edi
	movl $15, %esi
	call w_opt_temp
	leaq s_board3(%rip), %rsi
	call w_cstr
	movl $BOARD_TEMP2, %edi
	call sbox_read
	movl %eax, %ebx
	movl %eax, %edi
	movl $15, %esi
	call w_opt_temp
	leaq s_board4(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	shrl $16, %edi
	movl $15, %esi
	call w_opt_temp
	leaq s_board5(%rip), %rsi
	call w_cstr
	movl $STATUS_FAN2, %edi
	call sbox_read
	shrl $12, %eax
	andl $0xff, %eax
	movl %eax, %edi
	call w_dec
	leaq s_board6(%rip), %rsi
	call w_cstr
	movl $THERMAL_STATUS, %edi
	call sbox_read
	testl $1 << 31, %eax
	jz 6f
	shrl $22, %eax
	andl $0x1ff, %eax
	movl %eax, %edi
	call w_dec
	leaq s_c_nl(%rip), %rsi
	call w_cstr
	jmp 7f
6:	leaq s_na_nl(%rip), %rsi
	call w_cstr
7:	# core voltage: VR12 code
	leaq s_volt1(%rip), %rsi
	call w_cstr
	movl $COREVOLT, %edi
	call sbox_read
	andl $0xff, %eax
	jnz 8f
	leaq s_na_nl(%rip), %rsi
	call w_cstr
	jmp 9f
8:	decl %eax
	leal 250(%rax,%rax,4), %edi
	call w_dec
	leaq s_mv_nl(%rip), %rsi
	call w_cstr
9:	# core clock from COREFREQ and CURRENT_CLK_RATIO
	movl $SPAD_PLATFORM_INFO, %edi
	call spad
	movl %eax, %r14d
	leaq s_clk1(%rip), %rsi
	call w_cstr
	movl $COREFREQ, %edi
	call sbox_read
	movl %eax, %edi
	movl %r14d, %esi
	call w_khz
	leaq s_clk2(%rip), %rsi
	call w_cstr
	movl $CURRENT_CLK_RATIO, %edi
	call sbox_read
	movl %eax, %edi
	movl %r14d, %esi
	call w_khz
	leaq s_clk3(%rip), %rsi
	call w_cstr
	call w_str_end
	addq $48, %rsp
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
# w_opt_temp(edi = register half, esi = valid bit position): "NN C" when
# bit 15 (of the half) is set, "n/a" otherwise.
w_opt_temp:
	testl $1 << 15, %edi
	jz 1f
	andl $0x1ff, %edi
	call w_dec
	leaq s_c(%rip), %rsi
	jmp w_cstr
1:	leaq s_na(%rip), %rsi
	jmp w_cstr
# w_khz(edi = ratio register, esi = spad4): "NNNN MHz" from the PLL
# table (core_khz), or "n/a (code 0x...)".
w_khz:
	pushq %rbx
	movl %edi, %ebx
	andl $0xfff, %ebx
	movl %ebx, %edi
	call core_khz
	testl %edx, %edx
	jz 1f
	xorl %edx, %edx
	movl $1000, %ecx
	divq %rcx
	movq %rax, %rdi
	call w_dec
	leaq s_mhz(%rip), %rsi
	call w_cstr
	popq %rbx
	ret
1:	leaq s_na_code(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call w_hex
	movb $')', %dil
	call w_byte
	popq %rbx
	ret
	.globl core_khz
# core_khz(edi = 12-bit ratio, esi = spad4) -> rax, edx = 1 when the code
# is in Intel's PLL table: feedback bits 8:1 times 200 MHz over the
# feed-forward divider (bits 10:9 inverted: 0 is 1, 1 is 2, 2 is 4),
# scaled by 20 over the fused ICC divider.
core_khz:
	movl %edi, %ecx
	notl %ecx
	shrl $9, %ecx
	andl $3, %ecx		# fwd
	movl %edi, %eax
	shrl $1, %eax
	andl $0xff, %eax	# bck
	cmpl $8, %eax
	jb 8f
	movl $1, %r8d
	movl $16, %r9d
	testl %ecx, %ecx
	jz 1f
	movl $2, %r8d
	movl $15, %r9d
	cmpl $1, %ecx
	je 1f
	movl $4, %r8d
	cmpl $2, %ecx
	jne 8f
1:	cmpl %r9d, %eax
	ja 8f
	movl %esi, %ecx
	shrl $25, %ecx
	andl $0x1f, %ecx
	jnz 2f
	movl $ICC_NOMINAL_DIVIDER, %ecx
2:	imulq $200000, %rax, %rax
	xorl %edx, %edx
	divq %r8
	imulq $20, %rax, %rax
	xorl %edx, %edx
	divq %rcx
	movl $1, %edx
	ret
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret
STR s_die1, "die temperatures: "
STR s_die2, " C (max seen "
STR s_die3, " C)\n"
STR s_board1, "board: inlet "
STR s_board2, ", vccp regulator "
STR s_board3, ", gddr "
STR s_board4, ", gddr regulator "
STR s_board5, ", vddg regulator "
STR s_board6, " C, tmu die "
STR s_volt1, "core voltage: "
STR s_clk1, "core clock: "
STR s_clk2, " (COREFREQ), "
STR s_clk3, " (CURRENT_CLK_RATIO)\n"
STR s_na, "n/a"
STR s_na_nl, "n/a\n"
STR s_c, " C"
STR s_c_nl, " C\n"
STR s_mv_nl, " mV\n"
STR s_mhz, " MHz"
STR s_na_code, "n/a (code "
