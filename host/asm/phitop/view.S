# view.S: rendering (host/crates/phitop's view.rs): one frame as text
# with ANSI colours, sized to the terminal, from a slot's current
# snapshot and derived numbers. Lines are built in line_buf through the
# write cursor and fitted into frame_buf. See view.md.
	.include "defs.inc"

	.set RESET_LEN, 4
	.set DIM, 245

	.bss
	.p2align 3
	.globl opt_sort_mem, opt_show_kthreads, opt_color, opt_interval_ms, opt_error
opt_sort_mem:	.skip 8
opt_show_kthreads:	.skip 8
opt_color:	.skip 8
opt_interval_ms:	.skip 8
# The error shown in the status (a pointer to a NUL-terminated string,
# or 0).
opt_error:	.skip 8
	.globl frame_buf, frame_len
frame_len:	.skip 8
v_cols:	.skip 8
v_rows:	.skip 8
v_nlines:	.skip 8
v_slot:	.skip 8
v_snap:	.skip 8
v_derived:	.skip 8
v_last_color:	.skip 8
v_cw:	.skip 8
tmp_saved:	.skip 8
sort_n:	.skip 8
	.p2align 6
line_buf:	.skip LINECAP
tmp_buf:	.skip 256
sort_idx:	.skip SN_PROC_CAP * 4
frame_buf:	.skip FRAMECAP

	.section .rodata
s_reset:	.ascii "\033[0m"
s_fg:	.asciz "\033[38;5;"
s_home:	.ascii "\033[H"
s_clear_eol:	.ascii "\033[K"
levels:	.ascii " "
	.byte 0xe2, 0x96, 0x81, 0xe2, 0x96, 0x82, 0xe2, 0x96, 0x83, 0xe2, 0x96, 0x84
	.byte 0xe2, 0x96, 0x85, 0xe2, 0x96, 0x86, 0xe2, 0x96, 0x87, 0xe2, 0x96, 0x88
board_names:	.quad b_inlet, b_vccp, b_gddr, b_gddrvr, b_vddg, b_tmu
b_inlet:	.asciz "inlet"
b_vccp:	.asciz "vccp"
b_gddr:	.asciz "gddr"
b_gddrvr:	.asciz "gddr vr"
b_vddg:	.asciz "vddg"
b_tmu:	.asciz "tmu"
	.text

# ------------------------------------------------------------ lines

# line_begin: the cursor at line_buf.
line_begin:
	leaq line_buf(%rip), %rax
	movq %rax, wp(%rip)
	ret

# line_end: the line fitted to v_cols visible characters (escape
# sequences not counted, a colour that was open closed), "\e[K" after
# it in colour, a newline except after the last row in colour; nothing
# once v_rows lines are out.
line_end:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	movq v_nlines(%rip), %rax
	cmpq v_rows(%rip), %rax
	jae 9f
	leaq line_buf(%rip), %rsi
	movq wp(%rip), %r12		# end of the line
	leaq frame_buf(%rip), %rdi
	addq frame_len(%rip), %rdi
	xorl %r13d, %r13d		# width
	xorl %r14d, %r14d		# coloured
	xorl %ebx, %ebx			# in escape
1:	cmpq %r12, %rsi
	jae 5f
	movzbl (%rsi), %eax
	testl %ebx, %ebx
	jz 2f
	movb %al, (%rdi)
	incq %rdi
	incq %rsi
	cmpb $'m', %al
	jne 1b
	xorl %ebx, %ebx
	jmp 1b
2:	cmpb $0x1b, %al
	jne 3f
	movl $1, %ebx
	movl $1, %r14d
	movb %al, (%rdi)
	incq %rdi
	incq %rsi
	jmp 1b
3:	movl %eax, %ecx
	andl $0xc0, %ecx
	cmpl $0x80, %ecx
	je 4f			# a continuation byte: copied, not counted
	cmpq v_cols(%rip), %r13
	jae 5f
	incq %r13
4:	movb %al, (%rdi)
	incq %rdi
	incq %rsi
	jmp 1b
5:	testl %r14d, %r14d
	jz 6f
	movl $0x6d305b1b, (%rdi)	# "\e[0m"
	addq $4, %rdi
6:	cmpq $0, opt_color(%rip)
	je 7f
	movb $0x1b, (%rdi)
	movb $'[', 1(%rdi)
	movb $'K', 2(%rdi)
	addq $3, %rdi
	movq v_nlines(%rip), %rax
	incq %rax
	cmpq v_rows(%rip), %rax
	jae 8f
7:	movb $'\n', (%rdi)
	incq %rdi
8:	leaq frame_buf(%rip), %rax
	subq %rax, %rdi
	movq %rdi, frame_len(%rip)
	incq v_nlines(%rip)
9:	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# empty_line: one blank line.
empty_line:
	call line_begin
	jmp line_end

# frame_begin(edi = cols, esi = rows): a new frame, "\e[H" first in colour.
frame_begin:
	movq %rdi, v_cols(%rip)
	movq %rsi, v_rows(%rip)
	movq $0, v_nlines(%rip)
	movq $0, frame_len(%rip)
	cmpq $0, opt_color(%rip)
	je 1f
	leaq frame_buf(%rip), %rdi
	movb $0x1b, (%rdi)
	movb $'[', 1(%rdi)
	movb $'H', 2(%rdi)
	movq $3, frame_len(%rip)
1:	ret

# ------------------------------------------------------------ pieces

# w_fg(edi = colour): "\e[38;5;Nm".
w_fg:
	pushq %rdi
	leaq s_fg(%rip), %rsi
	call w_cstr
	popq %rdi
	call w_dec
	movb $'m', %dil
	jmp w_byte

# w_reset: "\e[0m".
w_reset:
	leaq s_reset(%rip), %rsi
	movl $4, %edx
	jmp w_bytes

# color_of(rdi = load in 1/10000) -> eax: grey, greens, yellow, orange, red.
color_of:
	movl $240, %eax
	cmpq $200, %rdi
	jb 1f
	movl $34, %eax
	cmpq $2500, %rdi
	jb 1f
	movl $70, %eax
	cmpq $5000, %rdi
	jb 1f
	movl $178, %eax
	cmpq $7500, %rdi
	jb 1f
	movl $208, %eax
	cmpq $9000, %rdi
	jb 1f
	movl $196, %eax
1:	ret

# w_level(rdi = load in 1/10000): the eighth block for it.
w_level:
	cmpq $10000, %rdi
	jbe 1f
	movl $10000, %edi
1:	movq %rdi, %rax			# round(load * 8 / 10000)
	shlq $3, %rax
	addq $5000, %rax
	xorl %edx, %edx
	movl $10000, %ecx
	divq %rcx
	cmpq $8, %rax
	jbe 2f
	movl $8, %eax
2:	testq %rax, %rax
	jnz 3f
	movb $' ', %dil
	jmp w_byte
3:	decq %rax
	leaq levels+1(%rip), %rsi
	leaq (%rax,%rax,2), %rax
	addq %rax, %rsi
	movl $3, %edx
	jmp w_bytes

# w_fixed_r(rdi = value, rsi = scale, edx = decimals): value / scale
# rounded to that many decimals ({:.N} of a float).
w_fixed_r:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rsi, %r12
	movl %edx, %r13d
	# half a unit of the last decimal: scale / (2 * 10^decimals)
	movq %r12, %rax
	movl %edx, %ecx
1:	testl %ecx, %ecx
	jz 2f
	xorl %edx, %edx
	movl $10, %ebx
	divq %rbx
	decl %ecx
	jmp 1b
2:	shrq $1, %rax
	addq %rax, %rdi
	movq %r12, %rsi
	movl %r13d, %edx
	call w_fixed
	popq %r13
	popq %r12
	popq %rbx
	ret

# w_rate(rdi = bytes per second): in a unit that fits.
w_rate:
	movq $1000000000, %rax
	cmpq %rax, %rdi
	jb 1f
	movq %rax, %rsi
	movl $2, %edx
	call w_fixed_r
	leaq s_gbs(%rip), %rsi
	jmp w_cstr
1:	cmpq $1000000, %rdi
	jb 2f
	movl $1000000, %esi
	movl $1, %edx
	call w_fixed_r
	leaq s_mbs(%rip), %rsi
	jmp w_cstr
2:	cmpq $1000, %rdi
	jb 3f
	movl $1000, %esi
	movl $1, %edx
	call w_fixed_r
	leaq s_kbs(%rip), %rsi
	jmp w_cstr
3:	call w_dec
	leaq s_bs(%rip), %rsi
	jmp w_cstr
STR s_gbs, " GB/s"
STR s_mbs, " MB/s"
STR s_kbs, " kB/s"
STR s_bs, " B/s"

# w_mib(rdi = kB): GiB with a decimal from 10 GiB, MiB from 10 MiB, KiB.
w_mib:
	cmpq $10 * 1024 * 1024, %rdi
	jb 1f
	movl $1048576, %esi
	movl $1, %edx
	call w_fixed_r
	leaq s_gib(%rip), %rsi
	jmp w_cstr
1:	cmpq $10 * 1024, %rdi
	jb 2f
	shrq $10, %rdi
	call w_dec
	leaq s_mib(%rip), %rsi
	jmp w_cstr
2:	call w_dec
	leaq s_kib(%rip), %rsi
	jmp w_cstr
STR s_gib, " GiB"
STR s_mib, " MiB"
STR s_kib, " KiB"

# w_bar(rdi = numerator, rsi = denominator, edx = width): "[###...]"
# with round(num / den * width) filled.
w_bar:
	pushq %rbx
	pushq %r12
	movl %edx, %r12d
	testq %rsi, %rsi
	jnz 1f
	movl $1, %esi
1:	movq %rdi, %rax
	cmpq %rsi, %rax
	jbe 2f
	movq %rsi, %rax
2:	imulq %r12, %rax
	addq %rax, %rax
	addq %rsi, %rax
	xorl %edx, %edx
	leaq (%rsi,%rsi), %rcx
	divq %rcx
	cmpq %r12, %rax
	jbe 3f
	movq %r12, %rax
3:	movq %rax, %rbx
	movb $'[', %dil
	call w_byte
	movl %ebx, %edi
	movb $'#', %sil
	call w_pad
	movl %r12d, %edi
	subl %ebx, %edi
	movb $'.', %sil
	call w_pad
	movb $']', %dil
	call w_byte
	popq %r12
	popq %rbx
	ret

# w_hms(rdi = ms): "h:mm:ss".
w_hms:
	pushq %rbx
	movq %rdi, %rax
	xorl %edx, %edx
	movl $1000, %ecx
	divq %rcx
	movq %rax, %rbx		# seconds
	xorl %edx, %edx
	movl $3600, %ecx
	divq %rcx
	movq %rax, %rdi
	call w_dec
	movb $':', %dil
	call w_byte
	movq %rbx, %rax
	xorl %edx, %edx
	movl $60, %ecx
	divq %rcx
	xorl %edx, %edx
	divq %rcx
	movq %rdx, %rdi
	movl $2, %esi
	movb $'0', %dl
	call w_decw
	movb $':', %dil
	call w_byte
	movq %rbx, %rax
	xorl %edx, %edx
	movl $60, %ecx
	divq %rcx
	movq %rdx, %rdi
	movl $2, %esi
	movb $'0', %dl
	call w_decw
	popq %rbx
	ret

# w_tenths(rdi = tenths): "N.d".
w_tenths:
	movl $10, %esi
	movl $1, %edx
	jmp w_fixed

# tmp_begin / tmp_right(edi = width): a piece built in tmp_buf, then
# right-aligned in width at the saved cursor.
tmp_begin:
	movq wp(%rip), %rax
	movq %rax, tmp_saved(%rip)
	leaq tmp_buf(%rip), %rax
	movq %rax, wp(%rip)
	ret
tmp_right:
	pushq %rbx
	movq wp(%rip), %rbx
	leaq tmp_buf(%rip), %rax
	subq %rax, %rbx			# the piece's length
	movq tmp_saved(%rip), %rax
	movq %rax, wp(%rip)
	subl %ebx, %edi
	jle 1f
	movb $' ', %sil
	call w_pad
1:	leaq tmp_buf(%rip), %rsi
	movq %rbx, %rdx
	call w_bytes
	popq %rbx
	ret

# w_left_pad(rsi = NUL-terminated, ecx = width): left-aligned in width.
w_left_pad:
	jmp w_left_cstr

# w_temp(edi = i16 as i64): "--" below zero, else the number.
w_temp:
	testq %rdi, %rdi
	js 1f
	jmp w_dec
1:	leaq s_dashes(%rip), %rsi
	jmp w_cstr
STR s_dashes, "--"

# ------------------------------------------------------------ lines of a frame

# temps_line(v_snap): "die   .. C   now N C  peak N C   core N MHz N mV   board ...".
temps_line:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq v_snap(%rip), %rbx
	leaq s_die(%rip), %rsi
	call w_cstr
	xorl %r12d, %r12d
	movq $-1, %r13			# the highest die temperature
1:	cmpl $9, %r12d
	jae 3f
	testl %r12d, %r12d
	jz 2f
	movb $' ', %dil
	call w_byte
2:	movswq SN_TEMPS(%rbx,%r12,2), %rdi
	cmpq SN_NTEMPS(%rbx), %r12
	jb 21f
	movq $-1, %rdi
21:	cmpq %r13, %rdi
	jle 22f
	movq %rdi, %r13
22:	call w_temp
	incl %r12d
	jmp 1b
3:	leaq s_c_now(%rip), %rsi
	call w_cstr
	movq %r13, %rdi
	call w_temp
	leaq s_c_peak(%rip), %rsi
	call w_cstr
	movq SN_PEAK(%rbx), %rdi
	call w_temp
	leaq s_c_core(%rip), %rsi
	call w_cstr
	movq SN_MHZ(%rbx), %rdi
	call w_dec
	leaq s_mhz_sp(%rip), %rsi
	call w_cstr
	movq SN_VCORE(%rbx), %rdi
	call w_dec
	leaq s_mv3(%rip), %rsi
	call w_cstr
	# the board sensors that read above 0: "name value" joined by two spaces
	xorl %r12d, %r12d
	xorl %r13d, %r13d		# any shown
4:	cmpl $6, %r12d
	jae 6f
	leal 9(%r12), %eax
	cmpq SN_NTEMPS(%rbx), %rax
	jae 6f
	movswq SN_TEMPS(%rbx,%rax,2), %rdi
	testq %rdi, %rdi
	jle 5f
	testl %r13d, %r13d
	jz 41f
	leaq s_two_sp(%rip), %rsi
	pushq %rdi
	call w_cstr
	popq %rdi
41:	movl $1, %r13d
	pushq %rdi
	leaq board_names(%rip), %rax
	movq (%rax,%r12,8), %rsi
	call w_cstr
	movb $' ', %dil
	call w_byte
	popq %rdi
	call w_dec
5:	incl %r12d
	jmp 4b
6:	testl %r13d, %r13d
	jnz 7f
	leaq s_board_na(%rip), %rsi
	call w_cstr
7:	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_die, "die   "
STR s_c_now, " C   now "
STR s_c_peak, " C  peak "
STR s_c_core, " C   core "
STR s_mhz_sp, " MHz "
STR s_mv3, " mV   "
STR s_two_sp, "  "
STR s_board_na, "board sensors: n/a (SMC)"

# rates_line(rsi = label, rdi = rates, rdx = count, r8 = a, r9 = b):
# "{label:<5} " then " name a rate b rate  " per device.
rates_line:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq %rdi, %rbx
	movq %rdx, %r12
	movq %r8, %r13
	movq %r9, %r14
	movl $5, %ecx
	call w_left_cstr
	movb $' ', %dil
	call w_byte
	xorl %r15d, %r15d
1:	cmpq %r12, %r15
	jae 9f
	movq %r15, %rax
	imulq $RATE_SIZE, %rax, %rax
	leaq (%rbx,%rax), %rdx
	pushq %rdx
	movb $' ', %dil
	call w_byte
	movq (%rsp), %rdx
	leaq RATE_NAME(%rdx), %rsi
	call w_cstr
	movb $' ', %dil
	call w_byte
	movq %r13, %rsi
	call w_cstr
	movb $' ', %dil
	call w_byte
	movq (%rsp), %rdx
	movq RATE_A(%rdx), %rdi
	call w_rate
	movb $' ', %dil
	call w_byte
	movq %r14, %rsi
	call w_cstr
	movb $' ', %dil
	call w_byte
	movq (%rsp), %rdx
	movq RATE_B(%rdx), %rdi
	call w_rate
	leaq s_two_sp(%rip), %rsi
	call w_cstr
	popq %rdx
	incq %r15
	jmp 1b
9:	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

# core_load(rdi = core entry; v_derived) -> rax = the mean load of its
# threads.
core_load:
	movq v_derived(%rip), %r8
	movzwl CORE_N(%rdi), %ecx
	xorl %eax, %eax
	xorl %edx, %edx
1:	cmpl %ecx, %edx
	jae 2f
	movzwl CORE_CPU(%rdi,%rdx,2), %r9d
	movl DV_CPU_LOAD(%r8,%r9,4), %r9d
	addq %r9, %rax
	incl %edx
	jmp 1b
2:	testl %ecx, %ecx
	jz 3f
	xorl %edx, %edx
	divq %rcx
3:	ret

# cell(rdi = load): the colour when it changed (v_last_color), the
# level, a space when cells are two wide.
cell:
	pushq %rdi
	cmpq $0, opt_color(%rip)
	je 1f
	call color_of
	cmpq v_last_color(%rip), %rax
	je 1f
	movq %rax, v_last_color(%rip)
	movl %eax, %edi
	call w_fg
1:	popq %rdi
	call w_level
	cmpq $2, v_cw(%rip)
	jne 2f
	movb $' ', %dil
	call w_byte
2:	ret

# grid: the "core" ruler, one row per hardware thread, the mean row.
grid:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	movq v_derived(%rip), %rbx
	movq DV_NCORES(%rbx), %r12
	# cell width: 2 when the ruler fits
	movq $2, v_cw(%rip)
	leaq (%r12,%r12), %rax
	addq $LABEL, %rax
	cmpq v_cols(%rip), %rax
	jbe 1f
	movq $1, v_cw(%rip)
1:	# the ruler: the number of every fifth (tenth) core at its column
	call line_begin
	leaq s_core(%rip), %rsi
	movl $LABEL, %ecx
	call w_left_cstr
	movq wp(%rip), %r13		# the ruler's start
	movq %r12, %rdi
	imulq v_cw(%rip), %rdi
	movb $' ', %sil
	call w_pad
	movl $5, %r14d
	cmpq $2, v_cw(%rip)
	je 2f
	movl $10, %r14d
2:	xorl %ecx, %ecx
3:	cmpq %r12, %rcx
	jae 6f
	movq %rcx, %rax
	imulq v_cw(%rip), %rax
	leaq (%r13,%rax), %rdi		# where the digits go
	movq wp(%rip), %r8
	# the digits of rcx, into a small buffer, then placed while they fit
	pushq %rcx
	pushq %rdi
	pushq %r8
	movq wp(%rip), %rax
	pushq %rax
	leaq tmp_buf(%rip), %rax
	movq %rax, wp(%rip)
	movq %rcx, %rdi
	call w_dec
	movq wp(%rip), %rdx
	leaq tmp_buf(%rip), %rsi
	subq %rsi, %rdx			# digit count
	popq %rax
	movq %rax, wp(%rip)
	popq %r8
	popq %rdi
	popq %rcx
4:	testq %rdx, %rdx
	jz 5f
	cmpq %r8, %rdi
	jae 5f
	movb (%rsi), %al
	movb %al, (%rdi)
	incq %rsi
	incq %rdi
	decq %rdx
	jmp 4b
5:	addq %r14, %rcx
	jmp 3b
6:	call line_end
	# one row per thread
	xorl %r13d, %r13d
7:	cmpq DV_MAXTHREADS(%rbx), %r13
	jae 10f
	call line_begin
	call tmp_begin
	movb $'t', %dil
	call w_byte
	movq %r13, %rdi
	call w_dec
	movq wp(%rip), %rdx
	leaq tmp_buf(%rip), %rsi
	subq %rsi, %rdx
	movq tmp_saved(%rip), %rax
	movq %rax, wp(%rip)
	movl $LABEL, %ecx
	call w_left
	movq $0, v_last_color(%rip)
	xorl %r14d, %r14d
8:	cmpq %r12, %r14
	jae 9f
	movq %r14, %rax
	imulq $CORE_SIZE, %rax, %rax
	leaq DV_CORES(%rbx,%rax), %rdi
	xorl %eax, %eax
	cmpw CORE_N(%rdi), %r13w
	jae 81f
	movzwl CORE_CPU(%rdi,%r13,2), %eax
	movl DV_CPU_LOAD(%rbx,%rax,4), %eax
81:	movq %rax, %rdi
	call cell
	incq %r14
	jmp 8b
9:	call line_end
	incq %r13
	jmp 7b
10:	# the mean row
	call line_begin
	leaq s_mean(%rip), %rsi
	movl $LABEL, %ecx
	call w_left_cstr
	movq $0, v_last_color(%rip)
	xorl %r14d, %r14d
11:	cmpq %r12, %r14
	jae 12f
	movq %r14, %rax
	imulq $CORE_SIZE, %rax, %rax
	leaq DV_CORES(%rbx,%rax), %rdi
	call core_load
	movq %rax, %rdi
	call cell
	incq %r14
	jmp 11b
12:	call line_end
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_core, "core"
STR s_mean, "mean"

# mean_row_only: the mean load per core as one line (the small block).
mean_row_only:
	pushq %rbx
	pushq %r12
	pushq %r14
	movq v_derived(%rip), %rbx
	movq DV_NCORES(%rbx), %r12
	movq $2, v_cw(%rip)
	leaq (%r12,%r12), %rax
	addq $LABEL, %rax
	cmpq v_cols(%rip), %rax
	jbe 1f
	movq $1, v_cw(%rip)
1:	call line_begin
	leaq s_mean(%rip), %rsi
	movl $LABEL, %ecx
	call w_left_cstr
	movq $0, v_last_color(%rip)
	xorl %r14d, %r14d
2:	cmpq %r12, %r14
	jae 3f
	movq %r14, %rax
	imulq $CORE_SIZE, %rax, %rax
	leaq DV_CORES(%rbx,%rax), %rdi
	call core_load
	movq %rax, %rdi
	call cell
	incq %r14
	jmp 2b
3:	call line_end
	popq %r14
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ processes

# sort_procs -> rax = the visible count: sort_idx holds the indexes of
# the listed processes (kernel threads only when shown), heap-sorted by
# CPU share then resident size, or resident size then share, descending.
sort_procs:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq v_snap(%rip), %rbx
	xorl %r12d, %r12d		# count
	xorl %r13d, %r13d
1:	cmpq SN_NLISTED(%rbx), %r13
	jae 3f
	cmpq $0, opt_show_kthreads(%rip)
	jne 2f
	movq %r13, %rax
	imulq $PR_SIZE, %rax, %rax
	cmpb $0, SN_PROCS+PR_KTHREAD(%rbx,%rax)
	jne 21f
2:	leaq sort_idx(%rip), %rax
	movl %r13d, (%rax,%r12,4)
	incq %r12
21:	incq %r13
	jmp 1b
3:	movq %r12, sort_n(%rip)
	# heapsort ascending by the comparator's reverse (so the largest ends
	# first): build a max-heap of "smaller" then extract
	cmpq $2, %r12
	jb 9f
	movq %r12, %r13
	shrq $1, %r13
4:	testq %r13, %r13
	jz 5f
	decq %r13
	movq %r13, %rdi
	movq %r12, %rsi
	call sift_down
	jmp 4b
5:	movq %r12, %r13
6:	cmpq $1, %r13
	jbe 9f
	decq %r13
	leaq sort_idx(%rip), %rax
	movl (%rax), %ecx
	movl (%rax,%r13,4), %edx
	movl %edx, (%rax)
	movl %ecx, (%rax,%r13,4)
	xorl %edi, %edi
	movq %r13, %rsi
	call sift_down
	jmp 6b
9:	movq %r12, %rax
	popq %r13
	popq %r12
	popq %rbx
	ret

# sift_down(rdi = root, rsi = heap size): a max-heap where "max" is the
# row that sorts last (smallest share), so extraction leaves the largest
# first.
sift_down:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rdi, %rbx
	movq %rsi, %r12
1:	leaq 1(%rbx,%rbx), %rcx		# left
	cmpq %r12, %rcx
	jae 9f
	movq %rcx, %r13			# the larger child
	leaq 1(%rcx), %rdx
	cmpq %r12, %rdx
	jae 2f
	leaq sort_idx(%rip), %rax
	movl (%rax,%rdx,4), %edi
	movl (%rax,%rcx,4), %esi
	call proc_after		# eax 1 when edi sorts after esi (smaller)
	testl %eax, %eax
	jz 2f
	leaq 2(%rbx,%rbx), %r13
2:	leaq sort_idx(%rip), %rax
	movl (%rax,%r13,4), %edi
	movl (%rax,%rbx,4), %esi
	call proc_after
	testl %eax, %eax
	jz 9f
	leaq sort_idx(%rip), %rax
	movl (%rax,%r13,4), %ecx
	movl (%rax,%rbx,4), %edx
	movl %edx, (%rax,%r13,4)
	movl %ecx, (%rax,%rbx,4)
	movq %r13, %rbx
	jmp 1b
9:	popq %r13
	popq %r12
	popq %rbx
	ret

# proc_after(edi = index a, esi = index b) -> eax 1 when a sorts after
# b: by share then size, or size then share, both descending.
proc_after:
	movq v_derived(%rip), %r8
	movq v_snap(%rip), %r9
	movl DV_PROC_CPU(%r8,%rdi,4), %eax	# cpu a
	movl DV_PROC_CPU(%r8,%rsi,4), %ecx	# cpu b
	movq %rdi, %r10
	imulq $PR_SIZE, %r10, %r10
	movq SN_PROCS+PR_RSS(%r9,%r10), %r10	# rss a
	movq %rsi, %r11
	imulq $PR_SIZE, %r11, %r11
	movq SN_PROCS+PR_RSS(%r9,%r11), %r11	# rss b
	cmpq $0, opt_sort_mem(%rip)
	jne 2f
	cmpl %ecx, %eax
	jb 8f
	ja 7f
	cmpq %r11, %r10
	jb 8f
	jmp 7f
2:	cmpq %r11, %r10
	jb 8f
	ja 7f
	cmpl %ecx, %eax
	jb 8f
7:	xorl %eax, %eax
	ret
8:	movl $1, %eax
	ret

# proc_row(rdi = index): "{:>7} {} {:>7.1} {:>9} {:>4}  {}", dimmed for
# a kernel thread in colour, its name in brackets.
proc_row:
	pushq %rbx
	pushq %r12
	movq %rdi, %r12
	movq v_snap(%rip), %rax
	movq %r12, %rbx
	imulq $PR_SIZE, %rbx, %rbx
	leaq SN_PROCS(%rax,%rbx), %rbx
	call line_begin
	cmpb $0, PR_KTHREAD(%rbx)
	je 1f
	cmpq $0, opt_color(%rip)
	je 1f
	movl $DIM, %edi
	call w_fg
1:	call tmp_begin
	movl PR_PID(%rbx), %edi
	call w_dec
	movl $7, %edi
	call tmp_right
	movb $' ', %dil
	call w_byte
	movzbl PR_STATE(%rbx), %edi
	call w_byte
	movb $' ', %dil
	call w_byte
	call tmp_begin
	movq v_derived(%rip), %rax
	movl DV_PROC_CPU(%rax,%r12,4), %edi
	call w_tenths
	movl $7, %edi
	call tmp_right
	movb $' ', %dil
	call w_byte
	call tmp_begin
	movq PR_RSS(%rbx), %rdi
	call w_mib
	movl $9, %edi
	call tmp_right
	movb $' ', %dil
	call w_byte
	call tmp_begin
	movl PR_THREADS(%rbx), %edi
	call w_dec
	movl $4, %edi
	call tmp_right
	leaq s_two_sp(%rip), %rsi
	call w_cstr
	cmpb $0, PR_KTHREAD(%rbx)
	je 2f
	movb $'[', %dil
	call w_byte
2:	leaq PR_COMM(%rbx), %rsi
	movl PR_COMM_LEN(%rbx), %edx
	call w_bytes
	cmpb $0, PR_KTHREAD(%rbx)
	je 3f
	movb $']', %dil
	call w_byte
3:	call line_end
	popq %r12
	popq %rbx
	ret

# proc_header(edi = 1 for the full frame's suffix with the kernel thread
# note).
proc_header:
	pushq %rdi
	call line_begin
	leaq s_ph1(%rip), %rsi
	call w_cstr
	popq %rdi
	testl %edi, %edi
	jz 2f
	leaq s_ph2(%rip), %rsi
	call w_cstr
	movq v_snap(%rip), %rax
	movl SN_NKTHREADS(%rax), %edi
	call w_dec
	leaq s_ph3(%rip), %rsi
	call w_cstr
	leaq s_none(%rip), %rsi
	cmpq $0, opt_show_kthreads(%rip)
	je 1f
	leaq s_the(%rip), %rsi
1:	call w_cstr
	leaq s_ph4(%rip), %rsi
	call w_cstr
2:	jmp line_end
STR s_ph1, "    PID S    CPU%       RSS  THR  COMMAND"
STR s_ph2, "   ("
STR s_ph3, " kernel threads, "
STR s_ph4, " active shown)"
STR s_none, "none"
STR s_the, "the"

# ------------------------------------------------------------ the frames

	.globl render
# render(rdi = slot, esi = cols, edx = rows): one card in full.
render:
	pushq %rbx
	pushq %r12
	pushq %r13
	movq %rdi, %rbx
	movq %rbx, v_slot(%rip)
	movq SL_PREV(%rbx), %rax
	movq %rax, v_snap(%rip)
	leaq SL_DERIVED(%rbx), %rax
	movq %rax, v_derived(%rip)
	movl %esi, %edi
	movl %edx, %esi
	call frame_begin
	movq v_snap(%rip), %r12
	movq v_derived(%rip), %r13
	# line 1
	call line_begin
	leaq s_h1(%rip), %rsi
	call w_cstr
	movq SN_UPTIME(%r12), %rdi
	call w_hms
	leaq s_h2(%rip), %rsi
	call w_cstr
	movzwl SN_LOAD(%r12), %edi
	call w_load
	movb $' ', %dil
	call w_byte
	movzwl SN_LOAD+2(%r12), %edi
	call w_load
	movb $' ', %dil
	call w_byte
	movzwl SN_LOAD+4(%r12), %edi
	call w_load
	leaq s_two_sp(%rip), %rsi
	call w_cstr
	movl SN_RUNNING(%r12), %edi
	call w_dec
	leaq s_h3(%rip), %rsi
	call w_cstr
	movl SN_NPROCS(%r12), %edi
	call w_dec
	leaq s_h4(%rip), %rsi
	call w_cstr
	movq SN_NCPU_SENT(%r12), %rdi
	call w_dec
	leaq s_h5(%rip), %rsi
	call w_cstr
	movq DV_MEAN(%r13), %rdi
	addq $5, %rdi
	xorl %edx, %edx
	movl $10, %ecx
	movq %rdi, %rax
	divq %rcx
	movq %rax, %rdi
	call w_tenths
	leaq s_h6(%rip), %rsi
	call w_cstr
	call status_interval2
	call line_end
	# line 2: temperatures
	call line_begin
	call temps_line
	call line_end
	# line 3: memory
	call line_begin
	leaq s_mem(%rip), %rsi
	call w_cstr
	movq SN_MEM(%r12), %rdi		# total
	subq SN_MEM+8(%r12), %rdi	# minus available
	jae 1f
	xorl %edi, %edi
1:	movq %rdi, %r8
	movq SN_MEM(%r12), %rsi
	movl $20, %edx
	pushq %r8
	call w_bar
	movb $' ', %dil
	call w_byte
	popq %rdi
	call w_mib
	leaq s_slash(%rip), %rsi
	call w_cstr
	movq SN_MEM(%r12), %rdi
	call w_mib
	leaq s_cache(%rip), %rsi
	call w_cstr
	movq SN_MEM+16(%r12), %rdi
	addq SN_MEM+24(%r12), %rdi
	call w_mib
	leaq s_swap(%rip), %rsi
	call w_cstr
	movq SN_MEM+32(%r12), %rdi
	subq SN_MEM+40(%r12), %rdi
	jae 2f
	xorl %edi, %edi
2:	pushq %rdi
	movq SN_MEM+32(%r12), %rsi
	movl $10, %edx
	call w_bar
	movb $' ', %dil
	call w_byte
	popq %rdi
	call w_mib
	leaq s_slash(%rip), %rsi
	call w_cstr
	movq SN_MEM+32(%r12), %rdi
	call w_mib
	call line_end
	# line 4: pcie
	call line_begin
	leaq s_pcie1(%rip), %rsi
	call w_cstr
	movq DV_DMA_TO(%r13), %rdi
	addq DV_APER_TO(%r13), %rdi
	call w_rate
	leaq s_pcie2(%rip), %rsi
	call w_cstr
	movq DV_DMA_TO(%r13), %rdi
	call w_rate
	leaq s_pcie3(%rip), %rsi
	call w_cstr
	movq DV_APER_TO(%r13), %rdi
	call w_rate
	leaq s_pcie4(%rip), %rsi
	call w_cstr
	movq DV_DMA_FROM(%r13), %rdi
	addq DV_APER_FROM(%r13), %rdi
	call w_rate
	leaq s_pcie2(%rip), %rsi
	call w_cstr
	movq DV_DMA_FROM(%r13), %rdi
	call w_rate
	leaq s_pcie3(%rip), %rsi
	call w_cstr
	movq DV_APER_FROM(%r13), %rdi
	call w_rate
	movb $')', %dil
	call w_byte
	call line_end
	# line 5: card devices
	call line_begin
	leaq s_card(%rip), %rsi
	leaq DV_DISKS(%r13), %rdi
	movq DV_NDISKS(%r13), %rdx
	leaq s_read(%rip), %r8
	leaq s_write(%rip), %r9
	call rates_line
	leaq s_empty(%rip), %rsi
	leaq DV_NETS(%r13), %rdi
	movq DV_NNETS(%r13), %rdx
	leaq s_rx(%rip), %r8
	leaq s_tx(%rip), %r9
	call rates_line
	call line_end
	call empty_line
	call grid
	call empty_line
	movl $1, %edi
	call proc_header
	# the rows that fit: rows - (lines so far + 1 footer)
	call sort_procs
	movq %rax, %r12			# visible
	movq v_rows(%rip), %rcx
	subq v_nlines(%rip), %rcx
	decq %rcx
	jns 3f
	xorl %ecx, %ecx
3:	cmpq %rcx, %r12
	jbe 4f
	movq %rcx, %r12
4:	xorl %r13d, %r13d
5:	cmpq %r12, %r13
	jae 6f
	leaq sort_idx(%rip), %rax
	movl (%rax,%r13,4), %edi
	call proc_row
	incq %r13
	jmp 5b
6:	# blank lines to the footer
	movq v_nlines(%rip), %rax
	incq %rax
	cmpq v_rows(%rip), %rax
	jae 7f
	call empty_line
	jmp 6b
7:	call line_begin
	leaq s_foot1(%rip), %rsi
	call w_cstr
	call w_sort_name
	leaq s_foot2(%rip), %rsi
	call w_cstr
	call w_kthreads_state
	leaq s_foot3(%rip), %rsi
	call w_cstr
	call line_end
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_h1, "phitop  Phi 3120A  up "
STR s_h2, "  load "
STR s_h3, " running of "
STR s_h4, " procs  "
STR s_h5, " threads "
STR s_h6, "% busy  "
STR s_mem, "mem   "
STR s_slash, " / "
STR s_cache, "  (cache "
STR s_swap, ")   swap on host RAM "
STR s_pcie1, "pcie  to card "
STR s_pcie2, " (dma "
STR s_pcie3, ", aperture "
STR s_pcie4, ")   from card "
STR s_card, "card"
STR s_empty, ""
STR s_read, "read"
STR s_write, "write"
STR s_rx, "rx"
STR s_tx, "tx"
STR s_foot1, "q quit   c/m sort by cpu/mem ("
STR s_foot2, ")   k kernel threads ("
STR s_foot3, ")   +/- interval"

# w_load(edi = hundredths): "{:.2}".
w_load:
	movl $100, %esi
	movl $2, %edx
	jmp w_fixed

# w_sort_name: "cpu" or "mem".
w_sort_name:
	leaq s_cpu(%rip), %rsi
	cmpq $0, opt_sort_mem(%rip)
	je 1f
	leaq s_memw(%rip), %rsi
1:	jmp w_cstr
STR s_cpu, "cpu"
STR s_memw, "mem"

# w_kthreads_state: "hidden" or "shown when active".
w_kthreads_state:
	leaq s_hidden(%rip), %rsi
	cmpq $0, opt_show_kthreads(%rip)
	je 1f
	leaq s_shown(%rip), %rsi
1:	jmp w_cstr
STR s_hidden, "hidden"
STR s_shown, "shown when active"

# status_interval2: the status: the error in red, else the sample
# interval "{:.2} s".
status_interval2:
	movq opt_error(%rip), %rsi
	testq %rsi, %rsi
	jz 2f
	cmpq $0, opt_color(%rip)
	je 1f
	pushq %rsi
	movl $196, %edi
	call w_fg
	popq %rsi
1:	jmp w_cstr
2:	movq v_derived(%rip), %rax
	movq DV_INTERVAL_US(%rax), %rdi
	movl $1000000, %esi
	movl $2, %edx
	call w_fixed_r
	leaq s_sp_s(%rip), %rsi
	jmp w_cstr
STR s_sp_s, " s"

# ------------------------------------------------------------ several cards

# card_block(rdi = slot, rsi = budget): one card's block, at most
# budget lines (the caller's row budget cuts the rest). The header says
# when a card has no sample.
card_block:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	movq %rdi, %rbx
	movq %rsi, %r12
	movq %rbx, v_slot(%rip)
	movq SL_PREV(%rbx), %rax
	movq %rax, v_snap(%rip)
	leaq SL_DERIVED(%rbx), %rax
	movq %rax, v_derived(%rip)
	movq v_nlines(%rip), %r14		# the block's first line
	call line_begin
	cmpq $0, opt_color(%rip)
	je 1f
	movl $75, %edi
	call w_fg
1:	leaq s_cardsp(%rip), %rsi
	call w_cstr
	movq SL_INDEX(%rbx), %rdi
	call w_dec
	movb $' ', %dil
	call w_byte
	leaq SL_NAME(%rbx), %rsi
	call w_cstr
	cmpq $0, opt_color(%rip)
	je 2f
	call w_reset
2:	leaq s_two_sp(%rip), %rsi
	call w_cstr
	leaq SL_BDF(%rbx), %rsi
	call w_cstr
	leaq s_two_sp(%rip), %rsi
	call w_cstr
	cmpq $0, SL_HAVE_DERIVED(%rbx)
	jne 3f
	leaq SL_ERROR(%rbx), %rsi
	cmpq $0, SL_ERROR_LEN(%rbx)
	jne 21f
	leaq s_nosample(%rip), %rsi
21:	call w_cstr
	call line_end
	jmp 99f
3:	movq v_snap(%rip), %r13
	leaq s_up(%rip), %rsi
	call w_cstr
	movq SN_UPTIME(%r13), %rdi
	call w_hms
	leaq s_h2(%rip), %rsi
	call w_cstr
	movzwl SN_LOAD(%r13), %edi
	call w_load
	leaq s_two_sp(%rip), %rsi
	call w_cstr
	movl SN_RUNNING(%r13), %edi
	call w_dec
	leaq s_h3(%rip), %rsi
	call w_cstr
	movl SN_NPROCS(%r13), %edi
	call w_dec
	leaq s_h4(%rip), %rsi
	call w_cstr
	movq SN_NCPU_SENT(%r13), %rdi
	call w_dec
	leaq s_h5(%rip), %rsi
	call w_cstr
	movq v_derived(%rip), %rax
	movq DV_MEAN(%rax), %rax
	addq $5, %rax
	xorl %edx, %edx
	movl $10, %ecx
	divq %rcx
	movq %rax, %rdi
	call w_tenths
	leaq s_busy(%rip), %rsi
	call w_cstr
	call line_end
	cmpq $3, %r12
	jb 99f
	cmpq $9, %r12
	jb 20f
	# the full block: temperatures, memory and pcie, the grid
	call line_begin
	call temps_line
	call line_end
	call line_begin
	leaq s_mem(%rip), %rsi
	call w_cstr
	movq SN_MEM(%r13), %rdi
	subq SN_MEM+8(%r13), %rdi
	jae 4f
	xorl %edi, %edi
4:	pushq %rdi
	movq SN_MEM(%r13), %rsi
	movl $12, %edx
	call w_bar
	movb $' ', %dil
	call w_byte
	popq %rdi
	call w_mib
	leaq s_slash(%rip), %rsi
	call w_cstr
	movq SN_MEM(%r13), %rdi
	call w_mib
	leaq s_swap2(%rip), %rsi
	call w_cstr
	movq SN_MEM+32(%r13), %rdi
	subq SN_MEM+40(%r13), %rdi
	jae 5f
	xorl %edi, %edi
5:	pushq %rdi
	movq SN_MEM+32(%r13), %rsi
	movl $6, %edx
	call w_bar
	movb $' ', %dil
	call w_byte
	popq %rdi
	call w_mib
	leaq s_slash(%rip), %rsi
	call w_cstr
	movq SN_MEM+32(%r13), %rdi
	call w_mib
	movb $' ', %dil
	call w_byte
	call w_pcie_short
	call line_end
	call grid
	# lines used: 3 + 2 + threads + 1
	movq v_derived(%rip), %rax
	movq DV_MAXTHREADS(%rax), %rcx
	addq $6, %rcx
	jmp 30f
20:	# the small block: the mean row, memory and pcie in one line
	call mean_row_only
	call line_begin
	leaq s_mem2(%rip), %rsi
	call w_cstr
	movq SN_MEM(%r13), %rdi
	subq SN_MEM+8(%r13), %rdi
	jae 21f
	xorl %edi, %edi
21:	call w_mib
	leaq s_slash(%rip), %rsi
	call w_cstr
	movq SN_MEM(%r13), %rdi
	call w_mib
	leaq s_swap3(%rip), %rsi
	call w_cstr
	movq SN_MEM+32(%r13), %rdi
	subq SN_MEM+40(%r13), %rdi
	jae 22f
	xorl %edi, %edi
22:	call w_mib
	leaq s_slash(%rip), %rsi
	call w_cstr
	movq SN_MEM+32(%r13), %rdi
	call w_mib
	call w_pcie_short
	call line_end
	movl $3, %ecx
30:	# room for processes: budget - (used + 1)
	movq %r12, %rax
	subq %rcx, %rax
	jbe 99f
	decq %rax
	jz 99f
	movq %rax, %r12			# room
	call sort_procs
	cmpq %r12, %rax
	jae 31f
	movq %rax, %r12
31:	xorl %edi, %edi
	call proc_header
	xorl %r13d, %r13d
32:	cmpq %r12, %r13
	jae 99f
	leaq sort_idx(%rip), %rax
	movl (%rax,%r13,4), %edi
	call proc_row
	incq %r13
	jmp 32b
99:	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_cardsp, "card "
STR s_nosample, "no sample yet"
STR s_up, "up "
STR s_busy, "% busy"
STR s_swap2, "   swap "
STR s_mem2, "mem "
STR s_swap3, "  swap "

# w_pcie_short: "   pcie to X from Y" (the block form uses two spaces
# before "pcie" in the small block; the full block three).
w_pcie_short:
	leaq s_pcie_to(%rip), %rsi
	call w_cstr
	movq v_derived(%rip), %rax
	movq DV_DMA_TO(%rax), %rdi
	addq DV_APER_TO(%rax), %rdi
	call w_rate
	leaq s_pcie_from(%rip), %rsi
	call w_cstr
	movq v_derived(%rip), %rax
	movq DV_DMA_FROM(%rax), %rdi
	addq DV_APER_FROM(%rax), %rdi
	jmp w_rate
STR s_pcie_to, "  pcie to "
STR s_pcie_from, " from "

	.globl render_multi
# render_multi(rdi = slots, rsi = count, edx = cols, ecx = rows): every
# card an equal share of the rows, blocks separated by a blank line,
# two footer lines.
render_multi:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	pushq %r15
	movq %rdi, %rbx
	movq %rsi, %r12
	movl %edx, %edi
	movl %ecx, %esi
	call frame_begin
	# per = (rows - 2 - (n - 1)) / n, at least 1
	movq v_rows(%rip), %rax
	subq $2, %rax
	jae 1f
	xorl %eax, %eax
1:	movq %r12, %rcx
	testq %rcx, %rcx
	jnz 2f
	movl $1, %ecx
2:	leaq -1(%rcx), %rdx
	subq %rdx, %rax
	jae 3f
	xorl %eax, %eax
3:	xorl %edx, %edx
	divq %rcx
	testq %rax, %rax
	jnz 4f
	movl $1, %eax
4:	movq %rax, %r13			# per
	xorl %r14d, %r14d
5:	cmpq %r12, %r14
	jae 8f
	testq %r14, %r14
	jz 6f
	call empty_line
6:	movq v_nlines(%rip), %r15	# the block's first line
	movq %r14, %rax
	imulq $SL_SIZE, %rax, %rax
	leaq (%rbx,%rax), %rdi
	movq %r13, %rsi
	# the block's own row budget: lines beyond per are dropped by
	# capping v_rows for its duration
	pushq v_rows(%rip)
	leaq (%r15,%r13), %rax
	cmpq v_rows(%rip), %rax
	jae 7f
	movq %rax, v_rows(%rip)
7:	call card_block
	popq v_rows(%rip)
	incq %r14
	jmp 5b
8:	# blank lines up to the footer
	movq v_nlines(%rip), %rax
	addq $2, %rax
	cmpq v_rows(%rip), %rax
	jae 9f
	call empty_line
	jmp 8b
9:	call line_begin
	leaq s_mf1(%rip), %rsi
	call w_cstr
	movq %r12, %rdi
	call w_dec
	leaq s_mf2(%rip), %rsi
	call w_cstr
	movq opt_error(%rip), %rsi
	testq %rsi, %rsi
	jz 10f
	cmpq $0, opt_color(%rip)
	je 91f
	pushq %rsi
	movl $196, %edi
	call w_fg
	popq %rsi
91:	call w_cstr
	jmp 11f
10:	movq opt_interval_ms(%rip), %rdi
	movl $1000, %esi
	movl $1, %edx
	call w_fixed_r
	leaq s_sp_s(%rip), %rsi
	call w_cstr
11:	call line_end
	call line_begin
	leaq s_mf3(%rip), %rsi
	call w_cstr
	call w_sort_name
	leaq s_foot2(%rip), %rsi
	call w_cstr
	call w_kthreads_state
	leaq s_foot3(%rip), %rsi
	call w_cstr
	call line_end
	popq %r15
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret
STR s_mf1, "phitop  "
STR s_mf2, " cards  every "
STR s_mf3, "q quit   n/p one card   a all   c/m sort by cpu/mem ("

	.globl render_waiting
# render_waiting(rdi = slot, esi = cols, edx = rows): the one-card frame
# before the first derivation: "sampling..." with a sample, else the
# error or "connecting...".
render_waiting:
	pushq %rbx
	movq %rdi, %rbx
	movl %esi, %edi
	movl %edx, %esi
	call frame_begin
	call line_begin
	cmpq $0, SL_HAVE_PREV(%rbx)
	je 1f
	leaq s_sampling(%rip), %rsi
	call w_cstr
	jmp 3f
1:	leaq SL_ERROR(%rbx), %rsi
	cmpq $0, SL_ERROR_LEN(%rbx)
	jne 2f
	leaq s_connecting(%rip), %rsi
2:	call w_cstr
3:	call line_end
	popq %rbx
	ret
STR s_sampling, "sampling..."
STR s_connecting, "connecting..."
