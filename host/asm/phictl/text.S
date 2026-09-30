# text.S: the byte-level helpers of phictl: a write cursor that every
# message, frame and path is built through, whole writes, standard output
# and error, number formatting and parsing, string comparison, the
# monotonic clock, sleeping, reading whole files. Calling convention is the
# System V one (arguments in rdi, rsi, rdx, rcx, r8, r9; rbx, rbp, r12 to
# r15 kept) unless a routine says otherwise. See text.md.
	.include "defs.inc"

	.bss
	.p2align 3
	.globl wp
wp:	.skip 8
str_at:	.skip 8
saved_wp:	.skip 8
	.globl out_buf, out_len
# Standard output, buffered until out_flush.
out_len:	.skip 8
path_saved:	.skip 8
	.globl path_buf
path_buf:	.skip PATHCAP
log_buf:	.skip LOGCAP
out_buf:	.skip LOGCAP
	.p2align 4
	.globl ts_scratch
ts_scratch:	.skip 16

	.section .rodata
s_oserr:	.asciz " (os error "
hexdig:	.asciz "0123456789abcdef"

	.text

# ------------------------------------------------------------ the cursor

	.globl w_byte
w_byte:
	movq wp(%rip), %rax
	movb %dil, (%rax)
	incq %rax
	movq %rax, wp(%rip)
	ret

	.globl w_u16
w_u16:
	movq wp(%rip), %rax
	movw %di, (%rax)
	addq $2, %rax
	movq %rax, wp(%rip)
	ret

	.globl w_u32
w_u32:
	movq wp(%rip), %rax
	movl %edi, (%rax)
	addq $4, %rax
	movq %rax, wp(%rip)
	ret

	.globl w_u64
w_u64:
	movq wp(%rip), %rax
	movq %rdi, (%rax)
	addq $8, %rax
	movq %rax, wp(%rip)
	ret

	.globl w_bytes
# w_bytes(rsi = ptr, rdx = len). Clobbers rdi, rsi, rcx.
w_bytes:
	movq wp(%rip), %rdi
	movq %rdx, %rcx
	rep movsb
	movq %rdi, wp(%rip)
	ret

	.globl w_cstr
# w_cstr(rsi): a NUL-terminated string without its NUL.
w_cstr:
	movq wp(%rip), %rdi
1:	movb (%rsi), %al
	testb %al, %al
	jz 2f
	movb %al, (%rdi)
	incq %rsi
	incq %rdi
	jmp 1b
2:	movq %rdi, wp(%rip)
	ret

	.globl w_dec
# w_dec(rdi): unsigned decimal.
w_dec:
	xorl %esi, %esi
	xorl %edx, %edx
	jmp w_num
	.globl w_decw
# w_decw(rdi, esi = width, dl = pad byte): right-aligned in width.
w_decw:
	.globl w_num
w_num:
	pushq %rbx
	subq $48, %rsp
	movl %esi, %ebx
	movb %dl, 40(%rsp)
	movq %rdi, %rax
	leaq 32(%rsp), %rcx
	movl $10, %r8d
1:	xorl %edx, %edx
	divq %r8
	addb $'0', %dl
	decq %rcx
	movb %dl, (%rcx)
	testq %rax, %rax
	jnz 1b
	leaq 32(%rsp), %rdx
	subq %rcx, %rdx
2:	cmpq %rbx, %rdx
	jae 3f
	decq %rcx
	movb 40(%rsp), %al
	movb %al, (%rcx)
	incq %rdx
	jmp 2b
3:	movq %rcx, %rsi
	call w_bytes
	addq $48, %rsp
	popq %rbx
	ret

	.globl w_sdec
# w_sdec(rdi): signed decimal.
w_sdec:
	testq %rdi, %rdi
	jns w_dec
	pushq %rdi
	movb $'-', %dil
	call w_byte
	popq %rdi
	negq %rdi
	jmp w_dec

	.globl w_hex
# w_hex(rdi): 0x and lowercase digits without leading zeros ({:#x}).
w_hex:
	xorl %esi, %esi
	jmp w_hexw
	.globl w_hexw
# w_hexw(rdi, esi = digits): 0x and exactly esi digits, zero padded when
# esi is not 0 ({:#010x} is esi 8).
w_hexw:
	pushq %rbx
	subq $32, %rsp
	movl %esi, %ebx
	movq %rdi, %rax
	leaq 24(%rsp), %rcx
	leaq hexdig(%rip), %r8
	xorl %r9d, %r9d
1:	movq %rax, %rdx
	andl $15, %edx
	movb (%r8,%rdx), %dl
	decq %rcx
	movb %dl, (%rcx)
	incl %r9d
	shrq $4, %rax
	jnz 1b
2:	cmpl %ebx, %r9d
	jae 3f
	decq %rcx
	movb $'0', (%rcx)
	incl %r9d
	jmp 2b
3:	movq wp(%rip), %rdi
	movb $'0', (%rdi)
	movb $'x', 1(%rdi)
	addq $2, %rdi
	movq %rdi, wp(%rip)
	movq %rcx, %rsi
	movl %r9d, %edx
	call w_bytes
	addq $32, %rsp
	popq %rbx
	ret

	.globl w_hex2
# w_hex2(edi): two lowercase hex digits ({:02x}).
w_hex2:
	leaq hexdig(%rip), %r8
	movl %edi, %eax
	shrl $4, %eax
	andl $15, %eax
	movb (%r8,%rax), %al
	movq wp(%rip), %rcx
	movb %al, (%rcx)
	movl %edi, %eax
	andl $15, %eax
	movb (%r8,%rax), %al
	movb %al, 1(%rcx)
	addq $2, %rcx
	movq %rcx, wp(%rip)
	ret

	.globl w_fixed
# w_fixed(rdi = value, esi = scale, edx = decimals): rdi / scale with
# that many decimals, as Rust prints {:.3} of a float from integer
# milliseconds (value in units of 1/scale).
w_fixed:
	pushq %rbx
	pushq %r12
	pushq %r13
	movl %esi, %r12d
	movl %edx, %r13d
	movq %rdi, %rax
	xorl %edx, %edx
	divq %r12
	movq %rdx, %rbx
	movq %rax, %rdi
	call w_dec
	testl %r13d, %r13d
	jz 9f
	movb $'.', %dil
	call w_byte
	# the fraction, r13 digits: rbx * 10^r13 / scale
	movq %rbx, %rax
	movl %r13d, %ecx
1:	imulq $10, %rax, %rax
	decl %ecx
	jnz 1b
	xorl %edx, %edx
	divq %r12
	movq %rax, %rdi
	movl %r13d, %esi
	movb $'0', %dl
	call w_decw
9:	popq %r13
	popq %r12
	popq %rbx
	ret

	.globl w_pad
# w_pad(edi = count, sil = byte): that many bytes.
w_pad:
	movq wp(%rip), %rax
1:	testl %edi, %edi
	jz 2f
	movb %sil, (%rax)
	incq %rax
	decl %edi
	jmp 1b
2:	movq %rax, wp(%rip)
	ret

	.globl w_left
# w_left(rsi = ptr, rdx = len, ecx = width): the bytes then spaces to width.
w_left:
	pushq %rcx
	pushq %rdx
	call w_bytes
	popq %rdx
	popq %rcx
	subl %edx, %ecx
	jle 1f
	movl %ecx, %edi
	movb $' ', %sil
	jmp w_pad
1:	ret

	.globl w_left_cstr
# w_left_cstr(rsi, ecx = width).
w_left_cstr:
	pushq %rcx
	call cstr_len
	popq %rcx
	jmp w_left

	.globl w_oserr
# w_oserr(edi = errno): the message and " (os error N)", Rust's io::Error.
w_oserr:
	pushq %rbx
	movl %edi, %ebx
	call errno_text
	movq %rax, %rsi
	call w_cstr
	leaq s_oserr(%rip), %rsi
	call w_cstr
	movl %ebx, %edi
	call w_dec
	movb $')', %dil
	call w_byte
	popq %rbx
	ret

	.globl w_str_begin
# w_str_begin / w_str_end: a wire string written in pieces; the end clips
# it to MAX_STRING bytes at a character boundary, as phi-rpc's clip.
w_str_begin:
	movq wp(%rip), %rax
	addq $2, %rax
	movq %rax, str_at(%rip)
	movq %rax, wp(%rip)
	ret

	.globl w_str_end
w_str_end:
	movq str_at(%rip), %rcx
	movq wp(%rip), %rax
	subq %rcx, %rax
	cmpq $MAX_STRING, %rax
	jbe 2f
	movl $MAX_STRING, %eax
1:	movb (%rcx,%rax), %dl
	andb $0xc0, %dl
	cmpb $0x80, %dl
	jne 3f
	decq %rax
	jmp 1b
3:	leaq (%rcx,%rax), %rdx
	movq %rdx, wp(%rip)
2:	movw %ax, -2(%rcx)
	ret

	.globl w_str
# w_str(rsi, rdx): a whole wire string.
w_str:
	pushq %rsi
	pushq %rdx
	call w_str_begin
	popq %rdx
	popq %rsi
	call w_bytes
	jmp w_str_end

	.globl w_str_cstr
w_str_cstr:
	call cstr_len
	jmp w_str

# ------------------------------------------------------------ output

	.globl write_all
# write_all(edi = fd, rsi, rdx) -> rax: 0, -errno, or 1 for a write of 0.
write_all:
	pushq %rbx
	pushq %r12
	pushq %r13
	movl %edi, %ebx
	movq %rsi, %r12
	movq %rdx, %r13
1:	testq %r13, %r13
	jz 3f
	movl %ebx, %edi
	movq %r12, %rsi
	movq %r13, %rdx
	SYS SYS_write
	testq %rax, %rax
	js 2f
	jz 4f
	addq %rax, %r12
	subq %rax, %r13
	jmp 1b
2:	cmpq $-EINTR, %rax
	je 1b
	jmp 5f
3:	xorl %eax, %eax
	jmp 5f
4:	movl $1, %eax
5:	popq %r13
	popq %r12
	popq %rbx
	ret

	.globl o_begin
# o_begin / o_end: a line of standard output, buffered; o_flush writes
# the buffer. The cursor is saved around it.
o_begin:
	movq wp(%rip), %rax
	movq %rax, saved_wp(%rip)
	leaq out_buf(%rip), %rax
	addq out_len(%rip), %rax
	movq %rax, wp(%rip)
	ret

	.globl o_end
# o_end: newline, then flush if the buffer is past half.
o_end:
	movb $'\n', %dil
	call w_byte
	.globl o_end_bare
o_end_bare:
	movq wp(%rip), %rax
	leaq out_buf(%rip), %rcx
	subq %rcx, %rax
	movq %rax, out_len(%rip)
	movq saved_wp(%rip), %rax
	movq %rax, wp(%rip)
	cmpq $LOGCAP / 2, out_len(%rip)
	jae o_flush
	ret

	.globl o_flush
o_flush:
	movq out_len(%rip), %rdx
	testq %rdx, %rdx
	jz 1f
	movl $1, %edi
	leaq out_buf(%rip), %rsi
	call write_all
	movq $0, out_len(%rip)
1:	ret

	.globl l_begin
# l_begin / l_end: a line on standard error, written at l_end. The cursor
# is saved around it, so a line can be logged mid-frame.
l_begin:
	movq wp(%rip), %rax
	movq %rax, saved_wp(%rip)
	leaq log_buf(%rip), %rax
	movq %rax, wp(%rip)
	ret

	.globl l_end
l_end:
	movb $'\n', %dil
	call w_byte
	.globl l_end_bare
l_end_bare:
	leaq log_buf(%rip), %rsi
	movq wp(%rip), %rdx
	subq %rsi, %rdx
	movl $2, %edi
	call write_all
	movq saved_wp(%rip), %rax
	movq %rax, wp(%rip)
	ret

	.globl l_tag
# l_tag: "[phictl] " at the cursor.
l_tag:
	leaq s_tag(%rip), %rsi
	jmp w_cstr
STR s_tag, "[phictl] "

	.globl l_elapsed
# l_elapsed(rdi = ms): "+   1.234s " as the Rust "+{:>8.3}s ".
l_elapsed:
	pushq %rbx
	movq %rdi, %rbx
	movb $'+', %dil
	call w_byte
	# width 8 right-aligned: digits of the whole part + 4
	movq %rbx, %rax
	xorl %edx, %edx
	movl $1000, %ecx
	divq %rcx
	movl $1, %ecx
1:	cmpq $10, %rax
	jb 2f
	xorl %edx, %edx
	movl $10, %esi
	divq %rsi
	incl %ecx
	jmp 1b
2:	movl $4, %edi
	subl %ecx, %edi
	jle 3f
	movb $' ', %sil
	call w_pad
3:	movq %rbx, %rdi
	movl $1000, %esi
	movl $3, %edx
	call w_fixed
	movb $'s', %dil
	call w_byte
	movb $' ', %dil
	call w_byte
	popq %rbx
	ret

	.globl fail
# fail(rsi = message): "phictl: <message>" on standard error, exit 1.
fail:
	pushq %rsi
	call l_begin
	leaq s_prefix(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	call l_end
	call o_flush
	movl $1, %edi
	SYS SYS_exit_group
STR s_prefix, "phictl: "

	.globl fail_os
# fail_os(rsi = what, edi = errno): "phictl: <what>: <message> (os error N)".
fail_os:
	pushq %rdi
	pushq %rsi
	call l_begin
	leaq s_prefix(%rip), %rsi
	call w_cstr
	popq %rsi
	call w_cstr
	leaq s_colon(%rip), %rsi
	call w_cstr
	popq %rdi
	call w_oserr
	call l_end
	call o_flush
	movl $1, %edi
	SYS SYS_exit_group
STR s_colon, ": "

	.globl p_begin
# p_begin / p_end: a NUL-terminated path in path_buf through the cursor;
# p_end returns it in rax.
p_begin:
	movq wp(%rip), %rax
	movq %rax, path_saved(%rip)
	leaq path_buf(%rip), %rax
	movq %rax, wp(%rip)
	ret

	.globl p_end
p_end:
	xorl %edi, %edi
	call w_byte
	movq path_saved(%rip), %rax
	movq %rax, wp(%rip)
	leaq path_buf(%rip), %rax
	ret

# ------------------------------------------------------------ strings

	.globl cstr_len
# cstr_len(rsi) -> rdx; rsi kept.
cstr_len:
	xorl %edx, %edx
1:	cmpb $0, (%rsi,%rdx)
	je 2f
	incq %rdx
	jmp 1b
2:	ret

	.globl eq_cstr
# eq_cstr(rsi, rdx = len, rdi = NUL-terminated) -> eax 1 when equal.
eq_cstr:
1:	movb (%rdi), %al
	testq %rdx, %rdx
	jz 2f
	testb %al, %al
	jz 3f
	cmpb (%rsi), %al
	jne 3f
	incq %rsi
	incq %rdi
	decq %rdx
	jmp 1b
2:	testb %al, %al
	jnz 3f
	movl $1, %eax
	ret
3:	xorl %eax, %eax
	ret

	.globl eq_cc
# eq_cc(rsi, rdi: both NUL-terminated) -> eax 1 when equal.
eq_cc:
	call cstr_len
	jmp eq_cstr

	.globl starts_cstr
# starts_cstr(rsi, rdx, rdi = NUL-terminated) -> eax 1 when the bytes
# start with the string; rsi and rdx then point past it.
starts_cstr:
1:	movb (%rdi), %al
	testb %al, %al
	jz 2f
	testq %rdx, %rdx
	jz 3f
	cmpb (%rsi), %al
	jne 3f
	incq %rsi
	incq %rdi
	decq %rdx
	jmp 1b
2:	movl $1, %eax
	ret
3:	xorl %eax, %eax
	ret

	.globl trim
# trim(rsi, rdx) -> rsi, rdx without leading and trailing ASCII whitespace.
trim:
1:	testq %rdx, %rdx
	jz 5f
	movzbl (%rsi), %ecx
	cmpb $' ', %cl
	je 2f
	subl $9, %ecx
	cmpl $4, %ecx
	ja 3f
2:	incq %rsi
	decq %rdx
	jmp 1b
3:	testq %rdx, %rdx
	jz 5f
	movzbl -1(%rsi,%rdx), %ecx
	cmpb $' ', %cl
	je 4f
	subl $9, %ecx
	cmpl $4, %ecx
	ja 5f
4:	decq %rdx
	jmp 3b
5:	ret

	.globl next_token
# next_token(rsi = cursor, rdi = end) -> rax = start (0 when none),
# rdx = length, rsi past it. Split on ASCII whitespace.
next_token:
1:	cmpq %rdi, %rsi
	jae 9f
	movzbl (%rsi), %ecx
	cmpb $' ', %cl
	je 2f
	subl $9, %ecx
	cmpl $4, %ecx
	ja 3f
2:	incq %rsi
	jmp 1b
3:	movq %rsi, %rax
4:	incq %rsi
	cmpq %rdi, %rsi
	jae 5f
	movzbl (%rsi), %ecx
	cmpb $' ', %cl
	je 5f
	subl $9, %ecx
	cmpl $4, %ecx
	ja 4b
5:	movq %rsi, %rdx
	subq %rax, %rdx
	ret
9:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl next_line
# next_line(rsi = cursor, rdi = end) -> rax = start (0 when none), rdx =
# length without the newline, rsi past the newline.
next_line:
	cmpq %rdi, %rsi
	jae 9f
	movq %rsi, %rax
1:	cmpq %rdi, %rsi
	jae 3f
	cmpb $'\n', (%rsi)
	je 2f
	incq %rsi
	jmp 1b
2:	movq %rsi, %rdx
	subq %rax, %rdx
	incq %rsi
	ret
3:	movq %rsi, %rdx
	subq %rax, %rdx
	ret
9:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl find_byte
# find_byte(rsi, rdx, cl = byte) -> rax = index, or -1.
find_byte:
	xorl %eax, %eax
1:	cmpq %rdx, %rax
	jae 2f
	cmpb (%rsi,%rax), %cl
	je 3f
	incq %rax
	jmp 1b
2:	movq $-1, %rax
3:	ret

# ------------------------------------------------------------ numbers

# digits(rsi, r9 = len > 0, r8d = base 10 or 16) -> rax, edx = 1 (ok) or 0.
digits:
	xorl %eax, %eax
1:	movzbl (%rsi), %ecx
	subl $'0', %ecx
	cmpl $9, %ecx
	jbe 2f
	cmpl $16, %r8d
	jne 8f
	movzbl (%rsi), %ecx
	orl $0x20, %ecx
	subl $'a', %ecx
	cmpl $5, %ecx
	ja 8f
	addl $10, %ecx
2:	mulq %r8
	jc 8f
	addq %rcx, %rax
	jc 8f
	incq %rsi
	decq %r9
	jnz 1b
	movl $1, %edx
	ret
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_u64
# parse_u64(rsi, rdx) -> rax, edx = 1 (ok) or 0: Rust's u64::from_str.
parse_u64:
	movq %rdx, %r9
	movl $10, %r8d
	testq %r9, %r9
	jz 8f
	cmpb $'+', (%rsi)
	jne digits
	incq %rsi
	decq %r9
	jnz digits
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_hex
# parse_hex(rsi, rdx) -> rax, edx: hex digits without a prefix
# (u64::from_str_radix(.., 16)).
parse_hex:
	movq %rdx, %r9
	movl $16, %r8d
	testq %r9, %r9
	jz 8f
	jmp digits
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_num
# parse_num(rsi, rdx) -> rax, edx: "0x..." hex or decimal, trimmed
# (phictl's parse_u64).
parse_num:
	call trim
	cmpq $2, %rdx
	jb parse_u64
	cmpb $'0', (%rsi)
	jne parse_u64
	movb 1(%rsi), %al
	orb $0x20, %al
	cmpb $'x', %al
	jne parse_u64
	addq $2, %rsi
	subq $2, %rdx
	jmp parse_hex

	.globl parse_u32
parse_u32:
	call parse_u64
	movl $0xffffffff, %ecx
	cmpq %rcx, %rax
	ja 8f
	ret
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_u16
parse_u16:
	call parse_u64
	cmpq $0xffff, %rax
	ja 8f
	ret
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_octal
# parse_octal(rsi, rdx) -> rax, edx.
parse_octal:
	movq %rdx, %r9
	movl $8, %r8d
	testq %r9, %r9
	jz 8f
	xorl %eax, %eax
1:	movzbl (%rsi), %ecx
	subl $'0', %ecx
	cmpl $7, %ecx
	ja 8f
	shlq $3, %rax
	addq %rcx, %rax
	incq %rsi
	decq %r9
	jnz 1b
	movl $1, %edx
	ret
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_size
# parse_size(rsi, rdx) -> rax, edx = 1: a size with an optional K, M or G
# suffix, a positive multiple of 2 MiB (phictl's parse_size).
parse_size:
	testq %rdx, %rdx
	jz 8f
	movzbl -1(%rsi,%rdx), %ecx
	orl $0x20, %ecx
	movl $0, %r10d
	cmpb $'k', %cl
	jne 1f
	movl $10, %r10d
	jmp 3f
1:	cmpb $'m', %cl
	jne 2f
	movl $20, %r10d
	jmp 3f
2:	cmpb $'g', %cl
	jne 4f
	movl $30, %r10d
3:	decq %rdx
4:	pushq %r10
	call parse_u64
	popq %rcx
	testl %edx, %edx
	jz 8f
	testq %rax, %rax
	jz 8f
	shlq %cl, %rax
	testq $(1 << 21) - 1, %rax
	jnz 8f
	movl $1, %edx
	ret
8:	xorl %eax, %eax
	xorl %edx, %edx
	ret

	.globl parse_ipv4
# parse_ipv4(rsi, rdx) -> eax = the address as four bytes in memory order
# (a.b.c.d with a in the low byte), edx = 1 or 0.
parse_ipv4:
	pushq %rbx
	pushq %r12
	pushq %r13
	xorl %ebx, %ebx		# result
	xorl %r13d, %r13d	# octets done
	leaq (%rsi,%rdx), %r12	# end
1:	# one octet: digits up to '.' or the end
	movq %rsi, %rdi
2:	cmpq %r12, %rdi
	jae 3f
	cmpb $'.', (%rdi)
	je 3f
	incq %rdi
	jmp 2b
3:	movq %rdi, %rdx
	subq %rsi, %rdx
	pushq %rdi
	call parse_u64
	popq %rdi
	testl %edx, %edx
	jz 8f
	cmpq $255, %rax
	ja 8f
	movl %r13d, %ecx
	shll $3, %ecx
	shll %cl, %eax
	orl %eax, %ebx
	incl %r13d
	leaq 1(%rdi), %rsi
	cmpq %r12, %rdi
	jae 4f
	cmpl $4, %r13d
	jae 8f
	jmp 1b
4:	cmpl $4, %r13d
	jne 8f
	movl %ebx, %eax
	movl $1, %edx
	jmp 9f
8:	xorl %eax, %eax
	xorl %edx, %edx
9:	popq %r13
	popq %r12
	popq %rbx
	ret

# ------------------------------------------------------------ time, sleep

	.globl now_ms
# now_ms() -> rax: CLOCK_MONOTONIC in milliseconds.
now_ms:
	movl $CLOCK_MONOTONIC, %edi
	leaq ts_scratch(%rip), %rsi
	SYS SYS_clock_gettime
	movq ts_scratch+8(%rip), %rax
	xorl %edx, %edx
	movl $1000000, %ecx
	divq %rcx
	imulq $1000, ts_scratch(%rip), %rcx
	addq %rcx, %rax
	ret

	.globl now_us
# now_us() -> rax: CLOCK_MONOTONIC in microseconds.
now_us:
	movl $CLOCK_MONOTONIC, %edi
	leaq ts_scratch(%rip), %rsi
	SYS SYS_clock_gettime
	movq ts_scratch+8(%rip), %rax
	xorl %edx, %edx
	movl $1000, %ecx
	divq %rcx
	imulq $1000000, ts_scratch(%rip), %rcx
	addq %rcx, %rax
	ret

	.globl now_ns_realtime
# now_ns_realtime() -> rax: CLOCK_REALTIME in nanoseconds.
now_ns_realtime:
	movl $CLOCK_REALTIME, %edi
	leaq ts_scratch(%rip), %rsi
	SYS SYS_clock_gettime
	imulq $1000000000, ts_scratch(%rip), %rax
	addq ts_scratch+8(%rip), %rax
	ret

	.globl sleep_us
# sleep_us(rdi = microseconds).
sleep_us:
	subq $16, %rsp
	movq %rdi, %rax
	xorl %edx, %edx
	movl $1000000, %ecx
	divq %rcx
	movq %rax, (%rsp)
	imulq $1000, %rdx, %rdx
	movq %rdx, 8(%rsp)
	movq %rsp, %rdi
	xorl %esi, %esi
	SYS SYS_nanosleep
	addq $16, %rsp
	ret

	.globl sleep_ms
sleep_ms:
	imulq $1000, %rdi, %rdi
	jmp sleep_us

	.globl poll_ms
# poll_ms(rdi = pollfds, esi = count, edx = ms) -> rax = ready (errors 0).
poll_ms:
	SYS 7
	testq %rax, %rax
	jns 1f
	xorl %eax, %eax
1:	ret

# ------------------------------------------------------------ files

	.globl read_file
# read_file(rdi = path, rsi = buf, rdx = cap) -> rax = length, or -errno
# (-EINVAL when the file is larger than cap).
read_file:
	pushq %rbx
	pushq %r12
	pushq %r13
	pushq %r14
	subq $8, %rsp
	movq %rsi, %r12
	movq %rdx, %r13
	movl $O_RDONLY | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	js 9f
	movl %eax, %ebx
	xorl %r14d, %r14d
1:	movq %r13, %rdx
	subq %r14, %rdx
	jz 4f
	movl %ebx, %edi
	leaq (%r12,%r14), %rsi
	SYS SYS_read
	testq %rax, %rax
	jz 3f
	js 2f
	addq %rax, %r14
	jmp 1b
2:	cmpq $-EINTR, %rax
	je 1b
	movq %rax, %r14
	jmp 3f
4:	movq $-EINVAL, %r14
3:	movl %ebx, %edi
	SYS SYS_close
	movq %r14, %rax
9:	addq $8, %rsp
	popq %r14
	popq %r13
	popq %r12
	popq %rbx
	ret

	.globl read_text
# read_text(rdi = path, rsi = buf, rdx = cap) -> rax = length, 0 when
# unreadable (fs::read_to_string(..).unwrap_or_default()).
read_text:
	call read_file
	testq %rax, %rax
	jns 1f
	xorl %eax, %eax
1:	ret

	.globl read_link
# read_link(rdi = path, rsi = buf, rdx = cap) -> rax = length or -errno.
read_link:
	SYS SYS_readlink
	ret

	.globl base_name
# base_name(rsi, rdx) -> rsi, rdx: the part after the last '/'.
base_name:
	movq %rdx, %rax
1:	testq %rax, %rax
	jz 2f
	cmpb $'/', -1(%rsi,%rax)
	je 3f
	decq %rax
	jmp 1b
3:	addq %rax, %rsi
	subq %rax, %rdx
2:	ret

	.globl map_file_whole
# map_file_whole(rdi = path) -> rax = a private copy-on-write mapping of
# the whole file (writes stay in this process), rdx = its length; rax =
# -errno on failure.
map_file_whole:
	pushq %rbx
	pushq %r12
	subq $152, %rsp
	movl $O_RDONLY | O_CLOEXEC, %esi
	xorl %edx, %edx
	SYS SYS_open
	testq %rax, %rax
	js 9f
	movl %eax, %ebx
	movl %ebx, %edi
	movq %rsp, %rsi
	SYS SYS_fstat
	testq %rax, %rax
	js 8f
	movq 48(%rsp), %r12		# st_size
	testq %r12, %r12
	jz 7f
	xorl %edi, %edi
	movq %r12, %rsi
	movl $PROT_READ | PROT_WRITE, %edx
	movl $MAP_PRIVATE, %r10d
	movl %ebx, %r8d
	xorl %r9d, %r9d
	SYS SYS_mmap
	cmpq $-4096, %rax
	ja 8f
	pushq %rax
	movl %ebx, %edi
	SYS SYS_close
	popq %rax
	movq %r12, %rdx
	jmp 9f
7:	movq $-EINVAL, %rax
8:	pushq %rax
	movl %ebx, %edi
	SYS SYS_close
	popq %rax
9:	addq $152, %rsp
	popq %r12
	popq %rbx
	ret

	.globl getenv
# getenv(rdi = NAME) -> rax = the value (NUL-terminated) or 0.
getenv:
	movq envp(%rip), %r8
1:	movq (%r8), %rsi
	testq %rsi, %rsi
	jz 9f
	addq $8, %r8
	movq %rdi, %rcx
2:	movb (%rcx), %al
	testb %al, %al
	jz 3f
	cmpb (%rsi), %al
	jne 1b
	incq %rcx
	incq %rsi
	jmp 2b
3:	cmpb $'=', (%rsi)
	jne 1b
	leaq 1(%rsi), %rax
	ret
9:	xorl %eax, %eax
	ret

	.globl errno_text
# errno_text(edi) -> rax: glibc's message for the errno values phictl can
# meet, "Unknown error" otherwise.
errno_text:
	leaq errno_table(%rip), %rcx
1:	movl (%rcx), %eax
	cmpl $-1, %eax
	je 2f
	cmpl %edi, %eax
	je 3f
	addq $16, %rcx
	jmp 1b
2:	leaq e_unknown(%rip), %rax
	ret
3:	movq 8(%rcx), %rax
	ret

	.section .rodata
	.p2align 3
errno_table:
	.long 1, 0;	.quad e1
	.long 2, 0;	.quad e2
	.long 4, 0;	.quad e4
	.long 5, 0;	.quad e5
	.long 6, 0;	.quad e6
	.long 9, 0;	.quad e9
	.long 11, 0;	.quad e11
	.long 12, 0;	.quad e12
	.long 13, 0;	.quad e13
	.long 14, 0;	.quad e14
	.long 16, 0;	.quad e16
	.long 17, 0;	.quad e17
	.long 19, 0;	.quad e19
	.long 20, 0;	.quad e20
	.long 21, 0;	.quad e21
	.long 22, 0;	.quad e22
	.long 24, 0;	.quad e24
	.long 25, 0;	.quad e25
	.long 28, 0;	.quad e28
	.long 32, 0;	.quad e32
	.long 36, 0;	.quad e36
	.long 95, 0;	.quad e95
	.long 98, 0;	.quad e98
	.long 104, 0;	.quad e104
	.long 111, 0;	.quad e111
	.long -1, 0;	.quad 0
e1:	.asciz "Operation not permitted"
e2:	.asciz "No such file or directory"
e4:	.asciz "Interrupted system call"
e5:	.asciz "Input/output error"
e6:	.asciz "No such device or address"
e9:	.asciz "Bad file descriptor"
e11:	.asciz "Resource temporarily unavailable"
e12:	.asciz "Cannot allocate memory"
e13:	.asciz "Permission denied"
e14:	.asciz "Bad address"
e16:	.asciz "Device or resource busy"
e17:	.asciz "File exists"
e19:	.asciz "No such device"
e20:	.asciz "Not a directory"
e21:	.asciz "Is a directory"
e22:	.asciz "Invalid argument"
e24:	.asciz "Too many open files"
e25:	.asciz "Inappropriate ioctl for device"
e28:	.asciz "No space left on device"
e32:	.asciz "Broken pipe"
e36:	.asciz "File name too long"
e95:	.asciz "Operation not supported"
e98:	.asciz "Address already in use"
e104:	.asciz "Connection reset by peer"
e111:	.asciz "Connection refused"
e_unknown:	.asciz "Unknown error"
	.text
